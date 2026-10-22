//! Logging: `tracing` to a size-rotated file (5 x 5 MB) plus stderr in debug builds.
//! The level can be changed at runtime from settings.

use std::path::Path;
use std::sync::OnceLock;

use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

use crate::settings::LogLevel;

const MAX_LOG_BYTES: usize = 5 * 1024 * 1024;
const MAX_LOG_FILES: usize = 5;

type ReloadHandle = reload::Handle<EnvFilter, Registry>;
static RELOAD: OnceLock<ReloadHandle> = OnceLock::new();

pub struct LogGuard {
    _guard: WorkerGuard,
}

fn filter_for(level: LogLevel) -> EnvFilter {
    let lvl = match level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    };
    // Keep noisy dependencies quiet unless explicitly tracing.
    EnvFilter::new(format!(
        "{lvl},tao=warn,wry=warn,tauri=warn,tracing=warn,hyper=warn,hyper_util=warn,\
         reqwest=warn,h2=warn,rustls=warn,tauri_plugin_updater=info,vido_lib={lvl}"
    ))
}

pub fn init(log_dir: &Path, level: LogLevel) -> LogGuard {
    let writer = FileRotate::new(
        log_dir.join("vido.log"),
        AppendCount::new(MAX_LOG_FILES),
        ContentLimit::BytesSurpassed(MAX_LOG_BYTES),
        Compression::None,
        None,
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(writer);
    let (filter, handle) = reload::Layer::new(filter_for(level));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true);

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);
    if cfg!(debug_assertions) {
        let _ = registry
            .with(fmt::layer().with_writer(std::io::stderr).with_target(true))
            .try_init();
    } else {
        let _ = registry.try_init();
    }
    let _ = RELOAD.set(handle);

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("?"),
            "panic at {location}: {msg}"
        );
    }));

    LogGuard { _guard: guard }
}

pub fn set_level(level: LogLevel) {
    if let Some(h) = RELOAD.get() {
        let _ = h.modify(|f| *f = filter_for(level));
    }
}
