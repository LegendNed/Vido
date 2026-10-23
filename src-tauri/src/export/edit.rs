//! The editor's "Save" and "Save & override".
//!
//! Both write the current edits with the editor's export options (preset,
//! resolution, size, codecs; "Original" copies the video when it can) through the
//! normal export queue. "Save" writes `<name>_edit.<ext>` next to the clip (or
//! where "Save as…" points); "Save & override" exports to a temporary file in the
//! clip's own container first and only then swaps it in, moving the original to
//! the Recycle Bin (see [`replace_original`]).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::probe::MediaInfo;
use super::{codec_ffmpeg_name, container_accepts_video, ExportRequest};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::{AudioCodec, Container, VideoCodec};

#[derive(Deserialize, Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SaveEditMode {
    /// Write `<name>_edit.<ext>` next to the original.
    New,
    /// Replace the original; it goes to the Recycle Bin.
    Override,
}

/// Marks the temporary output of an override; such files are not listed as clips.
pub const OVERRIDE_TEMP_TAG: &str = ".vido-tmp";

/// Recycle attempts while another process (thumbnailer, antivirus, a player)
/// still has the original open.
const RECYCLE_ATTEMPTS: u32 = 8;
const RENAME_ATTEMPTS: u32 = 5;
/// Pause between attempts.
pub const RETRY_DELAY: Duration = Duration::from_millis(250);

fn stem_of(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".into())
}

fn dir_of(p: &Path) -> &Path {
    p.parent().unwrap_or(Path::new("."))
}

/// `<stem>_edit.<ext>` next to `input` (`<stem>_edit (2).<ext>`, ... if taken).
pub fn edit_output_path(input: &Path, ext: &str) -> PathBuf {
    crate::util::unique_path(dir_of(input), &format!("{}_edit", stem_of(input)), ext)
}

/// Temporary `<stem>.vido-tmp.<ext>` next to `input` for an override. A
/// leftover from an earlier failed swap is never reused (it may be the only
/// copy of that edit).
pub fn override_temp_path(input: &Path) -> PathBuf {
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mp4".into());
    crate::util::unique_path(
        dir_of(input),
        &format!("{}{OVERRIDE_TEMP_TAG}", stem_of(input)),
        &ext,
    )
}

/// Whether a file name belongs to an in-progress (or stranded) override.
pub fn is_override_temp(file_name: &str) -> bool {
    file_name.to_lowercase().contains(OVERRIDE_TEMP_TAG)
}

/// Turns the editor's request into a save of `input`. "Save" keeps every option
/// and names the file `<name>_edit.<ext>` unless a path was picked; an override
/// keeps the clip's container (and so its name), switching the codecs only
/// where that container can't hold the chosen ones.
pub fn save_request(
    input: &Path,
    req: &ExportRequest,
    info: &MediaInfo,
    mode: SaveEditMode,
) -> AppResult<ExportRequest> {
    if info.video.is_none() {
        return Err(AppError::invalid(
            "inputPath",
            "the file has no video stream",
        ));
    }
    let mut r = ExportRequest {
        input_path: input.to_string_lossy().into_owned(),
        ..req.clone()
    };
    let output = match mode {
        SaveEditMode::New => match &r.output_path {
            Some(p) => PathBuf::from(p),
            None => edit_output_path(input, r.extension()),
        },
        SaveEditMode::Override if r.gif => {
            return Err(
                AppError::invalid("gif", "Save & override can't turn a clip into a GIF")
                    .with_hint("Use Save instead."),
            );
        }
        SaveEditMode::Override => {
            let ext = input
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let container = match ext.as_str() {
                "mp4" => Container::Mp4,
                "mkv" => Container::Mkv,
                "webm" => Container::Webm,
                _ => {
                    return Err(AppError::invalid(
                        "inputPath",
                        format!("Save & override is not available for .{ext} files"),
                    )
                    .with_hint("Use Save instead."))
                }
            };
            r.container = container;
            if !container_accepts_video(container, codec_ffmpeg_name(r.video_codec)) {
                r.video_codec = if container == Container::Webm {
                    VideoCodec::Av1
                } else {
                    VideoCodec::H264
                };
            }
            if container == Container::Webm {
                r.audio_codec = AudioCodec::Opus;
            }
            override_temp_path(input)
        }
    };
    r.output_path = Some(output.to_string_lossy().into_owned());
    Ok(r)
}

fn retry<T>(attempts: u32, delay: Duration, mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut n = 1;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if n >= attempts => return Err(e),
            Err(_) => {
                n += 1;
                std::thread::sleep(delay);
            }
        }
    }
}

fn exists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// Final step of "Save & override", once `edited` (a complete file next to
/// `original`) has been written: moves `original` to the Recycle Bin with
/// `recycle`, then gives `edited` the original's name and modification time
/// (so the clip keeps its date and place in the list).
///
/// Never loses data: if the original cannot be recycled, `edited` is deleted
/// and the original stays untouched; if the rename fails after recycling,
/// `edited` is kept and the error says where it is.
pub fn replace_original(
    original: &Path,
    edited: &Path,
    recycle: &dyn Fn(&Path) -> io::Result<()>,
    delay: Duration,
) -> AppResult<()> {
    let name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = std::fs::metadata(original).and_then(|m| m.modified()).ok();

    let recycled = retry(RECYCLE_ATTEMPTS, delay, || {
        // Gone already (e.g. an earlier attempt reported an error after moving it).
        if !exists(original) {
            return Ok(());
        }
        recycle(original)?;
        // The rename below replaces existing files: never let it hit the original.
        if exists(original) {
            return Err(io::Error::other("the file is still there"));
        }
        Ok(())
    });
    if let Err(e) = recycled {
        let _ = std::fs::remove_file(edited);
        return Err(AppError::new(
            ErrorCode::Io,
            format!("Could not move {name} to the Recycle Bin ({e})"),
        )
        .with_hint("The original was not changed. Close programs that use it and try again."));
    }

    let renamed = retry(RENAME_ATTEMPTS, delay, || {
        if exists(original) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "another file took its place",
            ));
        }
        std::fs::rename(edited, original)
    });
    if let Err(e) = renamed {
        return Err(AppError::new(
            ErrorCode::Io,
            format!(
                "{name} is in the Recycle Bin, but the edited clip could not take its name ({e})"
            ),
        )
        .with_hint(format!("The edited clip is saved as {}", edited.display())));
    }

    if let Some(t) = modified {
        if let Err(e) = std::fs::File::options()
            .write(true)
            .open(original)
            .and_then(|f| f.set_modified(t))
        {
            tracing::debug!("could not keep the date of {}: {e}", original.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::SystemTime;

    use super::super::probe::{ClipAudioTrack, VideoStream};
    use super::super::{plan, AudioOut, ExportAudioTrack, VideoPlan};
    use super::*;
    use crate::av::q;
    use crate::settings::{EncoderPreference, QualityPreset, RateMode, ScalePreset};

    fn info(codec: Option<VideoCodec>, codec_name: &str, audio_codec: &str) -> MediaInfo {
        MediaInfo {
            duration: 30.0,
            start_time: 0.0,
            format: "mov,mp4".into(),
            video: Some(VideoStream {
                index: 0,
                codec,
                codec_name: codec_name.into(),
                width: 2560,
                height: 1440,
                fps: 60.0,
                fps_q: q(60, 1),
                time_base: q(1, 15360),
            }),
            audio: ["Mix", "Desktop audio", "Microphone"]
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    (
                        i + 1,
                        ClipAudioTrack {
                            index: i as u32,
                            title: (*t).into(),
                            codec: audio_codec.into(),
                            channels: 2,
                            sample_rate: 48_000,
                            bitrate_kbps: 160,
                        },
                    )
                })
                .collect(),
        }
    }

    /// What the editor sends: its edits plus whatever the Export panel shows.
    fn editor_request(input: &Path) -> ExportRequest {
        ExportRequest {
            input_path: input.to_string_lossy().into_owned(),
            output_path: Some("C:\\elsewhere\\x.webm".into()),
            trim_start: 2.0,
            trim_end: 12.0,
            crop: None,
            blur: Vec::new(),
            vertical_fit: false,
            gif: false,
            speed: 1.0,
            scale: ScalePreset::P720,
            fps: 30,
            video_codec: VideoCodec::Av1,
            encoder: EncoderPreference::Nvenc,
            preset: QualityPreset::Speed,
            rate_mode: RateMode::TargetSize,
            quality: 30,
            bitrate_kbps: 50,
            max_bitrate_kbps: 0,
            target_size_mb: 10.0,
            audio_tracks: (0..3)
                .map(|i| ExportAudioTrack {
                    index: i,
                    include: true,
                    muted: false,
                    volume: 1.0,
                })
                .collect(),
            mix_audio: false,
            keep_mixed_tracks: false,
            fade_in: 0.0,
            fade_out: 0.0,
            audio_codec: AudioCodec::Aac,
            audio_bitrate_kbps: 64,
            container: Container::Webm,
            stream_copy: false,
        }
    }

    /// The "Original" preset: source size, stream copy, MP4, saved next to the clip.
    fn original_request(input: &Path) -> ExportRequest {
        ExportRequest {
            output_path: None,
            scale: ScalePreset::Native,
            fps: 0,
            video_codec: VideoCodec::H264,
            rate_mode: RateMode::Quality,
            quality: 18,
            bitrate_kbps: 8_000,
            audio_bitrate_kbps: 160,
            container: Container::Mp4,
            stream_copy: true,
            ..editor_request(input)
        }
    }

    const KF: [f64; 3] = [0.0, 2.0, 4.0];

    /// An existing input file (the planner refuses outputs that resolve to the input).
    fn clip(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn edit_output_names_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("Vido_2026-10-18.mp4");
        assert_eq!(
            edit_output_path(&input, "mp4").file_name().unwrap(),
            "Vido_2026-10-18_edit.mp4"
        );
        std::fs::write(dir.path().join("Vido_2026-10-18_edit.mp4"), b"x").unwrap();
        assert_eq!(
            edit_output_path(&input, "mp4").file_name().unwrap(),
            "Vido_2026-10-18_edit (2).mp4"
        );
    }

    #[test]
    fn override_temp_names_are_unique_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("clip.mkv");
        let tmp = override_temp_path(&input);
        assert_eq!(tmp.file_name().unwrap(), "clip.vido-tmp.mkv");
        assert!(is_override_temp("clip.vido-tmp.mkv"));
        assert!(!is_override_temp("clip.mkv"));
        // A stranded temp file from an earlier attempt is left alone.
        std::fs::write(&tmp, b"x").unwrap();
        let next = override_temp_path(&input);
        assert_eq!(next.file_name().unwrap(), "clip.vido-tmp (2).mkv");
        assert!(is_override_temp(
            &next.file_name().unwrap().to_string_lossy()
        ));
    }

    #[test]
    fn save_follows_the_export_options() {
        let dir = tempfile::tempdir().unwrap();
        let input = clip(&dir, "clip.mp4");
        let mut editor = editor_request(&input);
        editor.output_path = None;
        editor.speed = 2.0;
        editor.fade_in = 1.0;
        editor.audio_tracks[2].volume = 0.5;
        let r = save_request(
            &input,
            &editor,
            &info(Some(VideoCodec::Hevc), "hevc", "aac"),
            SaveEditMode::New,
        )
        .unwrap();
        // Edits and every export option survive; only the name is filled in.
        assert_eq!(
            ExportRequest {
                output_path: None,
                ..r.clone()
            },
            editor
        );
        assert_eq!(
            r.output_path.as_deref(),
            Some(dir.path().join("clip_edit.webm").to_string_lossy().as_ref())
        );
        // "Save as…" picks the place.
        let r = save_request(
            &input,
            &editor_request(&input),
            &info(Some(VideoCodec::Hevc), "hevc", "aac"),
            SaveEditMode::New,
        )
        .unwrap();
        assert_eq!(r.output_path.as_deref(), Some(r"C:\elsewhere\x.webm"));
    }

    #[test]
    fn trim_only_save_is_a_stream_copy() {
        let dir = tempfile::tempdir().unwrap();
        let input = clip(&dir, "clip.mp4");
        let i = info(Some(VideoCodec::H264), "h264", "aac");
        let r = save_request(&input, &original_request(&input), &i, SaveEditMode::New).unwrap();
        let p = plan(&r, &i, &KF).unwrap();
        assert_eq!(p.video, VideoPlan::Copy);
        assert!(p.audio.iter().all(|a| matches!(a, AudioOut::Copy { .. })));
        assert_eq!(p.output, dir.path().join("clip_edit.mp4"));
    }

    #[test]
    fn save_can_rebuild_the_mix_and_keep_the_sources() {
        // The editor's lanes changed: a new mix in front, every source after it.
        let dir = tempfile::tempdir().unwrap();
        let input = clip(&dir, "clip.mp4");
        let i = info(Some(VideoCodec::H264), "h264", "aac");
        let mut editor = original_request(&input);
        editor.mix_audio = true;
        editor.keep_mixed_tracks = true;
        editor.audio_tracks[0].include = false;
        editor.audio_tracks[2].muted = true;
        let r = save_request(&input, &editor, &i, SaveEditMode::New).unwrap();
        assert!(r.mix_audio && r.keep_mixed_tracks);
        let p = plan(&r, &i, &KF).unwrap();
        assert_eq!(p.video, VideoPlan::Copy);
        let titles: Vec<&str> = p
            .audio
            .iter()
            .map(|a| match a {
                AudioOut::Copy { title, .. } | AudioOut::Encode { title, .. } => title.as_str(),
            })
            .collect();
        assert_eq!(titles, ["Mix", "Desktop audio", "Microphone"]);
        assert!(
            matches!(&p.audio[0], AudioOut::Encode { inputs, .. } if inputs == &vec![(1, 1.0)])
        );
        assert!(matches!(p.audio[1], AudioOut::Copy { index: 1, .. }));
        assert!(
            matches!(&p.audio[2], AudioOut::Encode { inputs, .. } if inputs == &vec![(2, 0.0)])
        );
    }

    #[test]
    fn override_writes_a_temp_file_in_the_same_container() {
        let dir = tempfile::tempdir().unwrap();
        let input = clip(&dir, "clip.webm");
        let i = info(None, "vp9", "opus");
        let mut editor = original_request(&input);
        editor.video_codec = VideoCodec::Hevc;
        let r = save_request(&input, &editor, &i, SaveEditMode::Override).unwrap();
        assert_eq!(r.container, Container::Webm);
        assert_eq!(
            r.video_codec,
            VideoCodec::Av1,
            "VP9 is re-encoded as AV1 in WebM"
        );
        assert_eq!(r.audio_codec, AudioCodec::Opus);
        let p = plan(&r, &i, &KF).unwrap();
        assert_eq!(p.output, dir.path().join("clip.vido-tmp.webm"));
        assert_eq!(p.video, VideoPlan::Copy);
    }

    #[test]
    fn mov_saves_as_mp4_but_cannot_be_overridden() {
        let dir = tempfile::tempdir().unwrap();
        let input = clip(&dir, "clip.mov");
        let i = info(Some(VideoCodec::H264), "h264", "aac");
        let r = save_request(&input, &original_request(&input), &i, SaveEditMode::New).unwrap();
        assert_eq!(r.container, Container::Mp4);
        assert!(r.output_path.unwrap().ends_with("clip_edit.mp4"));
        let e =
            save_request(&input, &editor_request(&input), &i, SaveEditMode::Override).unwrap_err();
        assert_eq!(e.field.as_deref(), Some("inputPath"));
    }

    /// Stand-in Recycle Bin: moves files into `bin` instead of the real one.
    fn fake_bin(bin: &Path) -> impl Fn(&Path) -> io::Result<()> + '_ {
        move |p: &Path| std::fs::rename(p, bin.join(p.file_name().unwrap()))
    }

    struct Files {
        _dir: tempfile::TempDir,
        bin: PathBuf,
        original: PathBuf,
        edited: PathBuf,
    }

    fn files() -> Files {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let original = dir.path().join("clip.mp4");
        let edited = override_temp_path(&original);
        std::fs::write(&original, b"original").unwrap();
        std::fs::write(&edited, b"edited").unwrap();
        Files {
            bin,
            original,
            edited,
            _dir: dir,
        }
    }

    #[test]
    fn override_recycles_the_original_and_takes_its_name() {
        let f = files();
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(&f.original)
            .unwrap()
            .set_modified(old)
            .unwrap();
        replace_original(&f.original, &f.edited, &fake_bin(&f.bin), Duration::ZERO).unwrap();
        assert_eq!(std::fs::read(&f.original).unwrap(), b"edited");
        assert!(!f.edited.exists());
        assert_eq!(std::fs::read(f.bin.join("clip.mp4")).unwrap(), b"original");
        let kept = std::fs::metadata(&f.original).unwrap().modified().unwrap();
        assert_eq!(kept, old, "the clip keeps its date");
    }

    #[test]
    fn override_retries_while_the_original_is_busy() {
        let f = files();
        let calls = Cell::new(0);
        let bin = fake_bin(&f.bin);
        let flaky = |p: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(io::Error::other("in use"))
            } else {
                bin(p)
            }
        };
        replace_original(&f.original, &f.edited, &flaky, Duration::ZERO).unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(std::fs::read(&f.original).unwrap(), b"edited");
    }

    #[test]
    fn failed_recycle_keeps_the_original_and_drops_the_edit() {
        let f = files();
        let calls = Cell::new(0);
        let busy = |_: &Path| {
            calls.set(calls.get() + 1);
            Err(io::Error::other("the file is in use by another program"))
        };
        let e = replace_original(&f.original, &f.edited, &busy, Duration::ZERO).unwrap_err();
        assert_eq!(calls.get(), RECYCLE_ATTEMPTS);
        assert!(e.message.contains("Recycle Bin"), "{}", e.message);
        assert_eq!(std::fs::read(&f.original).unwrap(), b"original");
        assert!(!f.edited.exists());
    }

    #[test]
    fn recycle_that_leaves_the_file_never_overwrites_it() {
        let f = files();
        let noop = |_: &Path| Ok(());
        assert!(replace_original(&f.original, &f.edited, &noop, Duration::ZERO).is_err());
        assert_eq!(std::fs::read(&f.original).unwrap(), b"original");
        assert!(!f.edited.exists());
    }

    #[cfg(windows)]
    #[test]
    fn failed_rename_keeps_the_edit_and_says_where() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = files();
        // Hold the edited file open without FILE_SHARE_DELETE so it cannot be renamed.
        let lock = std::fs::File::options()
            .read(true)
            .share_mode(1)
            .open(&f.edited)
            .unwrap();
        let e = replace_original(&f.original, &f.edited, &fake_bin(&f.bin), Duration::ZERO)
            .unwrap_err();
        drop(lock);
        assert!(f.edited.exists(), "the edit is kept");
        assert_eq!(std::fs::read(f.bin.join("clip.mp4")).unwrap(), b"original");
        let hint = e.hint.unwrap_or_default();
        assert!(hint.contains(&*f.edited.to_string_lossy()), "{hint}");
    }
}
