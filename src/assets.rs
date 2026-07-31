//! Embedded (or dev-loaded) model and soundfont assets.
//!
//! Small text configs are always embedded via `include_str!`. The large binary
//! assets (447MB weights, 49MB soundfont) are embedded only under the `embed`
//! feature; without it they are read from the repo at runtime to keep dev builds
//! fast. Callers get a `Cow` so the embedded path stays zero-copy.

use std::borrow::Cow;

pub const CONFIG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/midi-model-tv2o-medium/config.json"
));

pub const GENERATION_CONFIG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/midi-model-tv2o-medium/generation_config.json"
));

#[cfg(feature = "embed")]
pub fn model_safetensors() -> Cow<'static, [u8]> {
    Cow::Borrowed(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/models/midi-model-tv2o-medium/model.safetensors"
    )))
}

#[cfg(not(feature = "embed"))]
pub fn model_safetensors() -> Cow<'static, [u8]> {
    Cow::Owned(read_dev_asset(
        "models/midi-model-tv2o-medium/model.safetensors",
    ))
}

#[cfg(feature = "embed")]
pub fn soundfont() -> Cow<'static, [u8]> {
    Cow::Borrowed(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/soundfont.sf2"
    )))
}

#[cfg(not(feature = "embed"))]
pub fn soundfont() -> Cow<'static, [u8]> {
    Cow::Owned(read_dev_asset("assets/soundfont.sf2"))
}

#[cfg(not(feature = "embed"))]
fn read_dev_asset(relative: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).unwrap_or_else(|err| {
        panic!(
            "required asset {} is not readable; run `bash scripts/download-assets.sh`: {err}",
            path.display()
        )
    })
}
