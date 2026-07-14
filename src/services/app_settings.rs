//! Persistent app settings (just the last save directory), mirroring the Python
//! `AppSettings`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{default_downloads_dir, settings_file};

#[derive(Serialize, Deserialize)]
struct Stored {
    save_directory: String,
}

pub struct AppSettings {
    save_directory: PathBuf,
}

impl AppSettings {
    pub fn load() -> Self {
        let fallback = default_downloads_dir();
        let save_directory = std::fs::read_to_string(settings_file())
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .map(|stored| PathBuf::from(stored.save_directory))
            .filter(|dir| dir.is_dir())
            .unwrap_or(fallback);
        Self { save_directory }
    }

    pub fn save_directory(&self) -> &Path {
        &self.save_directory
    }

    pub fn set_save_directory(&mut self, directory: PathBuf) {
        self.save_directory = directory;
        self.persist();
    }

    fn persist(&self) {
        let path = settings_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let stored = Stored {
            save_directory: self.save_directory.to_string_lossy().into_owned(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&stored) {
            std::fs::write(path, text).ok();
        }
    }
}
