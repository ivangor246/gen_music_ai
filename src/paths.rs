//! Writable filesystem locations. A single self-contained binary may live in a
//! read-only place, so user state goes to an XDG data dir (not next to the exe).

use std::path::PathBuf;

use directories::{ProjectDirs, UserDirs};

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "midi-model")
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".midi-model"))
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn presets_file() -> PathBuf {
    data_dir().join("presets.json")
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// Default directory offered for manual saves (~/Downloads, else home).
pub fn default_downloads_dir() -> PathBuf {
    if let Some(dirs) = UserDirs::new() {
        if let Some(downloads) = dirs.download_dir() {
            if downloads.is_dir() {
                return downloads.to_path_buf();
            }
        }
        return dirs.home_dir().to_path_buf();
    }
    data_dir()
}

/// Create the writable directories used at runtime.
pub fn ensure_runtime_directories() {
    std::fs::create_dir_all(cache_dir()).ok();
}
