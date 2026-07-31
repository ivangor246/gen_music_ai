//! Embedded (or dev-loaded) model and soundfont assets.
//!
//! Small text configs are always embedded via `include_str!`. The large binary
//! assets (447MB weights, 49MB soundfont) are embedded only under the `embed`
//! feature; without it they are read from the repo at runtime to keep dev builds
//! fast. Callers get a `Cow` so the embedded path stays zero-copy, while asset
//! access errors are returned to the UI instead of terminating the application.

use std::borrow::Cow;

#[cfg(not(feature = "embed"))]
use anyhow::Context;
use anyhow::Result;

pub const CONFIG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/midi-model-tv2o-medium/config.json"
));

pub const GENERATION_CONFIG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/midi-model-tv2o-medium/generation_config.json"
));

#[cfg(feature = "embed")]
pub fn model_safetensors() -> Result<Cow<'static, [u8]>> {
    Ok(Cow::Borrowed(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/models/midi-model-tv2o-medium/model.safetensors"
    ))))
}

#[cfg(not(feature = "embed"))]
pub fn model_safetensors() -> Result<Cow<'static, [u8]>> {
    read_dev_asset("models/midi-model-tv2o-medium/model.safetensors").map(Cow::Owned)
}

#[cfg(feature = "embed")]
pub fn soundfont() -> Result<Cow<'static, [u8]>> {
    Ok(Cow::Borrowed(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/soundfont.sf2"
    ))))
}

#[cfg(not(feature = "embed"))]
pub fn soundfont() -> Result<Cow<'static, [u8]>> {
    read_dev_asset("assets/soundfont.sf2").map(Cow::Owned)
}

#[cfg(not(feature = "embed"))]
fn read_dev_asset(relative: &str) -> Result<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).with_context(|| {
        format!(
            "required asset {} is not readable; run `bash scripts/download-assets.sh`",
            path.display()
        )
    })
}

#[cfg(all(test, not(feature = "embed")))]
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
