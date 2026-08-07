//! Persistent application settings: the last save directory and the precision
//! the checkpoint is loaded at.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{default_downloads_dir, settings_file};

#[derive(Serialize, Deserialize)]
struct Stored {
    save_directory: String,
    /// Defaulted so settings files written before this option stay readable.
    #[serde(default)]
    half_precision: bool,
}

pub struct AppSettings {
    save_directory: PathBuf,
    half_precision: bool,
}

impl AppSettings {
    pub fn load() -> Self {
        let stored = std::fs::read_to_string(settings_file())
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok());
        let half_precision = stored.as_ref().is_some_and(|stored| stored.half_precision);
        let save_directory = stored
            .map(|stored| PathBuf::from(stored.save_directory))
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(default_downloads_dir);
        Self {
            save_directory,
            half_precision,
        }
    }

    pub fn save_directory(&self) -> &Path {
        &self.save_directory
    }

    pub fn set_save_directory(&mut self, directory: PathBuf) {
        self.save_directory = directory;
        self.persist();
    }

    /// Load the checkpoint in f16: half the memory, and half the bytes read per
    /// decoded event.
    pub fn half_precision(&self) -> bool {
        self.half_precision
    }

    pub fn set_half_precision(&mut self, enabled: bool) {
        self.half_precision = enabled;
        self.persist();
    }

    fn persist(&self) {
        let path = settings_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let stored = Stored {
            save_directory: self.save_directory.to_string_lossy().into_owned(),
            half_precision: self.half_precision,
        };
        if let Ok(text) = serde_json::to_string_pretty(&stored) {
            std::fs::write(path, text).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings written before the precision option must keep loading, or a
    /// user's save directory silently resets on upgrade.
    #[test]
    fn settings_without_the_precision_field_still_load() {
        let stored: Stored = serde_json::from_str(r#"{"save_directory": "/tmp"}"#).unwrap();
        assert_eq!(stored.save_directory, "/tmp");
        assert!(!stored.half_precision);
    }

    #[test]
    fn precision_survives_a_round_trip() {
        let written = serde_json::to_string(&Stored {
            save_directory: "/tmp".to_string(),
            half_precision: true,
        })
        .unwrap();
        let read: Stored = serde_json::from_str(&written).unwrap();
        assert!(read.half_precision);
        assert_eq!(read.save_directory, "/tmp");
    }
}
