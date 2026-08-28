//! Embedded (or dev-loaded) SoundFont asset.
//!
//! Release builds embed the 49MB SoundFont. Development builds read it from the
//! repository so ordinary code changes do not repeatedly process the asset.
//! Model checkpoints are managed independently by `services::model_store`.

use std::borrow::Cow;

#[cfg(not(feature = "embed-soundfont"))]
use anyhow::Context;
use anyhow::Result;

#[cfg(feature = "embed-soundfont")]
pub fn soundfont() -> Result<Cow<'static, [u8]>> {
    Ok(Cow::Borrowed(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/soundfont.sf2"
    ))))
}

#[cfg(not(feature = "embed-soundfont"))]
pub fn soundfont() -> Result<Cow<'static, [u8]>> {
    read_dev_asset("assets/soundfont.sf2").map(Cow::Owned)
}

#[cfg(not(feature = "embed-soundfont"))]
fn read_dev_asset(relative: &str) -> Result<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).with_context(|| {
        format!(
            "required asset {} is not readable; run `bash scripts/download-assets.sh`",
            path.display()
        )
    })
}

#[cfg(all(test, not(feature = "embed-soundfont")))]
mod tests {
    use super::*;

    #[test]
    fn missing_asset_returns_recovery_instructions() {
        let error = read_dev_asset("missing-test-asset").unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("missing-test-asset"));
        assert!(message.contains("scripts/download-assets.sh"));
    }
}
