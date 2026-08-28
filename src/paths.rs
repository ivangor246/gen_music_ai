//! Writable filesystem locations. User state lives in the platform-specific
//! application data directory rather than next to the executable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::{ProjectDirs, UserDirs};

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const LEGACY_APP_NAME: &str = "midi-model";

fn data_dir_for(application: &str) -> PathBuf {
    ProjectDirs::from("", "", application)
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!(".{application}")))
}

pub fn data_dir() -> PathBuf {
    data_dir_for(APP_NAME)
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn presets_file() -> PathBuf {
    data_dir().join("presets.json")
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// Default directory offered for manual saves (~/Downloads, else home).
pub fn default_downloads_dir() -> PathBuf {
    let Some(dirs) = UserDirs::new() else {
        return data_dir();
    };
    if let Some(downloads) = dirs.download_dir()
        && downloads.is_dir()
    {
        return downloads.to_path_buf();
    }
    dirs.home_dir().to_path_buf()
}

pub fn ensure_runtime_directories() -> Result<()> {
    migrate_directory(&data_dir_for(LEGACY_APP_NAME), &data_dir())?;
    for directory in [cache_dir(), models_dir()] {
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("creating {}", directory.display()))?;
    }
    Ok(())
}

fn migrate_directory(legacy: &Path, current: &Path) -> Result<bool> {
    if legacy == current || !legacy.exists() || current.exists() {
        return Ok(false);
    }
    if let Some(parent) = current
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::rename(legacy, current).with_context(|| {
        format!(
            "moving application data from {} to {}",
            legacy.display(),
            current.display()
        )
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_data_once() {
        let root = std::env::temp_dir().join(format!("path_migration_test_{}", std::process::id()));
        let legacy = root.join("legacy");
        let current = root.join("current");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("presets.json"), "[]").unwrap();

        assert!(migrate_directory(&legacy, &current).unwrap());
        assert!(!legacy.exists());
        assert!(current.join("presets.json").is_file());
        assert!(!migrate_directory(&legacy, &current).unwrap());

        std::fs::remove_dir_all(&root).ok();
    }
}
