//! Generation settings and request, mirroring the Python `settings.py`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const AUTO_VALUE: &str = "Автоматически";
pub const TICKS_PER_QUARTER: i64 = 480;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationSettings {
    #[serde(default)]
    pub instruments: Vec<String>,
    #[serde(default = "default_drum_kit")]
    pub drum_kit: String,
    #[serde(default = "default_bpm")]
    pub bpm: u16,
    #[serde(default = "default_time_signature")]
    pub time_signature: String,
    #[serde(default = "default_key_signature")]
    pub key_signature: String,
    #[serde(default = "default_bars")]
    pub bars: u32,
    #[serde(default = "default_events_per_bar")]
    pub events_per_bar: u32,
    #[serde(default)]
    pub context_window: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_true")]
    pub allow_control_changes: bool,
}

fn default_drum_kit() -> String {
    "Нет".to_string()
}
fn default_bpm() -> u16 {
    120
}
fn default_time_signature() -> String {
    "4/4".to_string()
}
fn default_key_signature() -> String {
    AUTO_VALUE.to_string()
}
fn default_bars() -> u32 {
    16
}
fn default_events_per_bar() -> u32 {
    24
}
fn default_temperature() -> f32 {
    1.0
}
fn default_top_p() -> f32 {
    0.94
}
fn default_top_k() -> usize {
    20
}
fn default_true() -> bool {
    true
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            instruments: Vec::new(),
            drum_kit: default_drum_kit(),
            bpm: default_bpm(),
            time_signature: default_time_signature(),
            key_signature: default_key_signature(),
            bars: default_bars(),
            events_per_bar: default_events_per_bar(),
            context_window: 0,
            temperature: default_temperature(),
            top_p: default_top_p(),
            top_k: default_top_k(),
            allow_control_changes: true,
        }
    }
}

impl GenerationSettings {
    /// numerator/denominator parsed from `time_signature` (e.g. "3/4").
    pub fn time_signature_parts(&self) -> (u32, u32) {
        let mut parts = self.time_signature.split('/');
        let numerator = parts.next().and_then(|s| s.parse().ok()).unwrap_or(4);
        let denominator = parts.next().and_then(|s| s.parse().ok()).unwrap_or(4);
        (numerator, denominator)
    }

    pub fn event_count(&self) -> u32 {
        (self.bars * self.events_per_bar).max(1)
    }

    pub fn estimated_seconds(&self) -> f64 {
        let (numerator, denominator) = self.time_signature_parts();
        let quarters_per_bar = numerator as f64 * 4.0 / denominator as f64;
        self.bars as f64 * quarters_per_bar * 60.0 / self.bpm.max(1) as f64
    }

    pub fn target_ticks(&self) -> i64 {
        let (numerator, denominator) = self.time_signature_parts();
        let quarters = self.bars as f64 * numerator as f64 * 4.0 / denominator as f64;
        (quarters * TICKS_PER_QUARTER as f64).round() as i64
    }
}

#[derive(Debug, Clone)]
pub struct GenerationRequest {
    pub settings: GenerationSettings,
    pub batch_size: usize,
    pub seed: u64,
    pub random_seed: bool,
    pub source_midi: Option<Vec<u8>>,
    pub source_event_count: usize,
    pub reduce_control_changes: bool,
    pub remap_track_channel: bool,
    pub add_default_instrument: bool,
    pub remove_empty_channels: bool,
    pub continuation_paths: Option<Vec<PathBuf>>,
    pub continuation_index: Option<usize>,
}

impl Default for GenerationRequest {
    fn default() -> Self {
        Self {
            settings: GenerationSettings::default(),
            batch_size: 4,
            seed: 0,
            random_seed: true,
            source_midi: None,
            source_event_count: 128,
            reduce_control_changes: true,
            remap_track_channel: true,
            add_default_instrument: true,
            remove_empty_channels: false,
            continuation_paths: None,
            continuation_index: None,
        }
    }
}
