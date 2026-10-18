//! Application state managed by Tauri.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::audio::wasapi::DeviceWatcher;
use crate::clips::ClipLibrary;
use crate::encode::CapabilityCache;
use crate::engine::Engine;
use crate::export::manager::ExportManager;
use crate::paths::AppPaths;
use crate::settings::store::SettingsStore;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: SettingsStore,
    pub engine: Engine,
    pub clips: ClipLibrary,
    pub exports: ExportManager,
    pub caps: Arc<CapabilityCache>,
    pub device_watcher: Mutex<Option<DeviceWatcher>>,
}

impl AppState {
    pub fn output_dir(&self) -> PathBuf {
        Engine::output_dir(&self.settings.get(), &self.paths)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.settings
            .get()
            .advanced
            .cache_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.paths.default_cache_dir.clone())
    }
}

pub fn thumbs_dir(paths: &AppPaths) -> PathBuf {
    paths.default_cache_dir.join("thumbs")
}
