//! Atomic file write: write to a hidden temp file in the target directory, then
//! rename over the destination. Mirrors the Python `TrackExporter`.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};

pub fn atomic_write(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).ok();
    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let temp = parent.join(format!(".{stem}.{}.tmp", std::process::id()));

    let mut file = File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
    match write(&mut file) {
        Ok(()) => {
            file.sync_all().ok();
            drop(file);
            std::fs::rename(&temp, path)
                .with_context(|| format!("renaming into {}", path.display()))?;
            Ok(())
        }
        Err(err) => {
            drop(file);
            std::fs::remove_file(&temp).ok();
            Err(err)
        }
    }
}
