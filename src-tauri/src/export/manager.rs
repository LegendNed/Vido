//! Export job queue: one worker thread (exports compete for the same encoder),
//! throttled progress events, cancellation and target-size retries.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Sender};
use parking_lot::Mutex;

use super::bitrate::corrected_bitrate;
use super::{edit, transcode};
use super::{ExportFailed, ExportProgress, ExportResult, ExportStage, Plan, VideoPlan};
use crate::encode::video::Rate;
use crate::encode::CapabilityCache;
use crate::error::{AppError, ErrorCode};

pub enum ExportEvent {
    Progress(ExportProgress),
    Completed(ExportResult),
    Failed(ExportFailed),
}

pub type EventSink = Arc<dyn Fn(ExportEvent) + Send + Sync>;

/// Runs on the export worker when a job succeeded, right before its completion event.
pub type OnDone = Box<dyn FnOnce(&ExportResult) + Send>;

/// What happens to the output file once the export succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// Leave it at the planned output path.
    Keep,
    /// Re-encode-on-save: replace the input file with the result.
    ReplaceInput,
    /// Editor "Save & override": the output is a temporary file next to the
    /// input; it takes the input's name and the input goes to the Recycle Bin.
    Override,
}

struct Job {
    id: String,
    plan: Plan,
    cancel: Arc<AtomicBool>,
    finish: Finish,
    on_done: Option<OnDone>,
}

pub struct ExportManager {
    tx: Sender<Job>,
    cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

const MAX_ATTEMPTS: u32 = 3;

impl ExportManager {
    pub fn new(caps: Arc<CapabilityCache>, sink: EventSink) -> Self {
        let (tx, rx) = unbounded::<Job>();
        let cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::default();
        let c2 = cancels.clone();
        std::thread::Builder::new()
            .name("vido-export".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let id = job.id.clone();
                    let sink2 = sink.clone();
                    let caps2 = caps.clone();
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_job(job, &caps2, &sink2)
                    }));
                    if outcome.is_err() {
                        sink(ExportEvent::Failed(ExportFailed {
                            job_id: id.clone(),
                            error: AppError::internal("the export crashed (see log)"),
                        }));
                    }
                    c2.lock().remove(&id);
                }
            })
            .expect("spawn export worker");
        Self { tx, cancels }
    }

    pub fn submit(&self, plan: Plan, replace_input: bool) -> String {
        let finish = if replace_input {
            Finish::ReplaceInput
        } else {
            Finish::Keep
        };
        self.submit_with(plan, finish)
    }

    pub fn submit_with(&self, plan: Plan, finish: Finish) -> String {
        self.submit_then(plan, finish, None)
    }

    /// Like [`Self::submit_with`]; `on_done` runs once the job succeeded.
    pub fn submit_then(&self, plan: Plan, finish: Finish, on_done: Option<OnDone>) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().insert(id.clone(), cancel.clone());
        let _ = self.tx.send(Job {
            id: id.clone(),
            plan,
            cancel,
            finish,
            on_done,
        });
        id
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.cancels.lock().get(id) {
            Some(c) => {
                c.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        for c in self.cancels.lock().values() {
            c.store(true, Ordering::SeqCst);
        }
    }
}

fn run_job(job: Job, caps: &CapabilityCache, sink: &EventSink) {
    let Job {
        id,
        mut plan,
        cancel,
        finish,
        on_done,
    } = job;
    if cancel.load(Ordering::SeqCst) {
        sink(ExportEvent::Failed(ExportFailed {
            job_id: id,
            error: AppError::new(ErrorCode::Cancelled, "export cancelled"),
        }));
        return;
    }
    let caps_now = caps.cached();
    let out_dur = plan.output_duration();
    let mut attempt = 1;
    let started = Instant::now();
    loop {
        let stage = if attempt == 1 {
            ExportStage::Encoding
        } else {
            ExportStage::Retrying
        };
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let mut last_pct = -1.0;
        let id2 = id.clone();
        let mut progress = |frac: f64, fps: f64| {
            let pct = (frac * 100.0).clamp(0.0, 100.0);
            if pct - last_pct >= 0.5 && last_emit.elapsed() >= Duration::from_millis(120)
                || frac >= 1.0
            {
                last_emit = Instant::now();
                last_pct = pct;
                let elapsed = started.elapsed().as_secs_f64();
                let eta = if frac > 0.01 {
                    elapsed / frac * (1.0 - frac)
                } else {
                    0.0
                };
                sink(ExportEvent::Progress(ExportProgress {
                    job_id: id2.clone(),
                    stage,
                    percent: pct,
                    fps,
                    eta_seconds: eta,
                    attempt,
                }));
            }
        };
        tracing::info!(
            "export {id} attempt {attempt}: {} → {} ({:?})",
            plan.input.display(),
            plan.output.display(),
            plan.video
        );
        match transcode::run(&plan, caps_now.as_ref(), &cancel, &mut progress) {
            Ok(stats) => {
                // Target size: retry with a corrected bitrate if we overshot.
                if let (
                    Some(target),
                    VideoPlan::Encode {
                        rate: Rate::Bitrate { kbps, .. },
                        ..
                    },
                ) = (plan.target_bytes, &plan.video)
                {
                    let over = stats.bytes > target + target / 100;
                    if over && attempt < MAX_ATTEMPTS {
                        let next = corrected_bitrate(*kbps, target, stats.bytes);
                        tracing::info!(
                            "export {id}: {} bytes > target {target}; retrying at {next} kbps",
                            stats.bytes
                        );
                        if let VideoPlan::Encode { rate, .. } = &mut plan.video {
                            *rate = Rate::Bitrate {
                                kbps: next,
                                max_kbps: next * 3 / 2,
                                bufsize_kbps: next * 2,
                            };
                        }
                        attempt += 1;
                        continue;
                    }
                }
                let mut output: PathBuf = plan.output.clone();
                match finish {
                    Finish::Keep => {}
                    Finish::ReplaceInput => {
                        let target = plan.input.with_extension(plan.container.extension());
                        if (std::fs::remove_file(&plan.input).is_ok() || !plan.input.exists())
                            && std::fs::rename(&plan.output, &target).is_ok()
                        {
                            output = target;
                        }
                    }
                    Finish::Override => {
                        sink(ExportEvent::Progress(ExportProgress {
                            job_id: id.clone(),
                            stage: ExportStage::Finalizing,
                            percent: 100.0,
                            fps: 0.0,
                            eta_seconds: 0.0,
                            attempt,
                        }));
                        if let Err(e) = edit::replace_original(
                            &plan.input,
                            &plan.output,
                            &crate::util::move_to_recycle_bin,
                            edit::RETRY_DELAY,
                        ) {
                            tracing::error!("export {id}: override failed: {}", e.message);
                            sink(ExportEvent::Failed(ExportFailed {
                                job_id: id,
                                error: e,
                            }));
                            return;
                        }
                        output = plan.input.clone();
                    }
                }
                let over_target = plan.target_bytes.is_some_and(|t| stats.bytes > t + t / 100);
                let result = ExportResult {
                    job_id: id,
                    output_path: output.to_string_lossy().into_owned(),
                    size_bytes: stats.bytes,
                    duration_seconds: out_dur,
                    video_copied: plan.video == VideoPlan::Copy,
                    encoder: stats.encoder,
                    attempts: attempt,
                    over_target,
                };
                if let Some(f) = on_done {
                    f(&result);
                }
                sink(ExportEvent::Completed(result));
                return;
            }
            Err(e) => {
                if e.code != ErrorCode::Cancelled {
                    tracing::error!("export {id} failed: {}", e.message);
                }
                sink(ExportEvent::Failed(ExportFailed {
                    job_id: id,
                    error: e,
                }));
                return;
            }
        }
    }
}
