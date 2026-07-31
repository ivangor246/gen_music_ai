//! Application state and the form -> settings mapping.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use iced::widget::canvas;

use crate::core::model::midi_model::MidiModel;
use crate::services::app_settings::AppSettings;
use crate::services::generation::{GeneratedTrack, KEY_SIGNATURES};
use crate::services::playback::PlaybackEngine;
use crate::services::presets::PresetStore;
use crate::services::timeline::Timeline;
use crate::settings::{AUTO_VALUE, GenerationRequest, GenerationSettings};

pub const MAX_INSTRUMENTS: usize = 15;
const MAX_BPM: u16 = 383;
const MAX_RESULTS: usize = 4;
const MAX_TOP_K: usize = crate::core::tokenizer::vocab::VOCAB_SIZE as usize;

pub const DRUM_KITS: [&str; 9] = [
    "None",
    "Standard",
    "Room",
    "Power",
    "Electronic",
    "TR-808",
    "Jazz",
    "Brush",
    "Orchestra",
];

pub const TIME_SIGNATURES: [&str; 14] = [
    "4/4", "2/4", "3/4", "6/4", "7/4", "2/2", "3/2", "4/2", "3/8", "5/8", "6/8", "7/8", "9/8",
    "12/8",
];

pub const CONTEXT_WINDOWS: [&str; 6] = [AUTO_VALUE, "256", "512", "1024", "2048", "4096"];

pub enum ModelState {
    NotLoaded,
    Loading,
    Ready(Arc<MidiModel>),
    Failed(String),
}

pub struct State {
    pub model: ModelState,
    pub status: String,

    // Form
    pub instruments: Vec<bool>,
    pub drum_kit: String,
    pub time_signature: String,
    pub key_signature: String,
    pub bpm: String,
    pub bars: String,
    pub events_per_bar: String,
    pub context_window: String,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: String,
    pub batch: String,
    pub seed: String,
    pub random_seed: bool,
    pub allow_cc: bool,

    // Presets
    pub preset_store: PresetStore,
    pub selected_preset: Option<String>,
    pub new_preset_name: String,

    // Generation
    pub generating: bool,
    pub cancel: Arc<AtomicBool>,
    pub progress: f32,

    // Results
    pub results: Vec<GeneratedTrack>,
    pub result_durations: Vec<f64>,
    pub seed_used: u64,
    pub selected_result: Option<usize>,
    pub confirming_cache_clear: bool,

    // Player
    pub player: Option<PlaybackEngine>,
    pub timeline: Option<Timeline>,
    pub density: Vec<f32>,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    pub density_cache: canvas::Cache,

    // Settings
    pub app_settings: AppSettings,
}

impl State {
    pub fn new() -> Self {
        let status = crate::paths::ensure_runtime_directories()
            .map(|()| "Ready.".to_string())
            .unwrap_or_else(|error| format!("Could not prepare application data: {error:#}"));
        Self {
            model: ModelState::NotLoaded,
            status,
            instruments: vec![false; 128],
            drum_kit: "None".to_string(),
            time_signature: "4/4".to_string(),
            key_signature: AUTO_VALUE.to_string(),
            bpm: "120".to_string(),
            bars: "16".to_string(),
            events_per_bar: "24".to_string(),
            context_window: AUTO_VALUE.to_string(),
            temperature: 1.0,
            top_p: 0.94,
            top_k: "20".to_string(),
            batch: "4".to_string(),
            seed: "0".to_string(),
            random_seed: true,
            allow_cc: true,
            preset_store: PresetStore::load(),
            selected_preset: None,
            new_preset_name: String::new(),
            generating: false,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: 0.0,
            results: Vec::new(),
            result_durations: Vec::new(),
            seed_used: 0,
            selected_result: None,
            confirming_cache_clear: false,
            player: None,
            timeline: None,
            density: Vec::new(),
            duration: 0.0,
            position: 0.0,
            playing: false,
            density_cache: canvas::Cache::new(),
            app_settings: AppSettings::load(),
        }
    }

    pub fn model(&self) -> Option<Arc<MidiModel>> {
        match &self.model {
            ModelState::Ready(model) => Some(model.clone()),
            _ => None,
        }
    }

    pub fn selected_instrument_count(&self) -> usize {
        self.instruments
            .iter()
            .filter(|selected| **selected)
            .count()
    }

    /// Names of currently selected instruments, in program order.
    pub fn selected_instruments(&self) -> Vec<String> {
        self.instruments
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .take(MAX_INSTRUMENTS)
            .map(|(index, _)| crate::core::midi::gm::PATCH_NAMES[index].to_string())
            .collect()
    }

    pub fn settings(&self) -> Result<GenerationSettings, String> {
        let (bpm, bars, events_per_bar) = self.composition_dimensions()?;
        if !DRUM_KITS.contains(&self.drum_kit.as_str()) {
            return Err("Drum Kit has an unsupported value.".to_string());
        }
        if !TIME_SIGNATURES.contains(&self.time_signature.as_str()) {
            return Err("Time Signature has an unsupported value.".to_string());
        }
        if self.key_signature != AUTO_VALUE
            && !KEY_SIGNATURES.contains(&self.key_signature.as_str())
        {
            return Err("Key Signature has an unsupported value.".to_string());
        }
        let context_window = if self.context_window == AUTO_VALUE {
            0
        } else {
            let parsed = parse_number::<u32>("Musical Memory", &self.context_window)?;
            if !CONTEXT_WINDOWS.contains(&self.context_window.as_str()) {
                return Err("Musical Memory has an unsupported value.".to_string());
            }
            parsed
        };
        if !(0.1..=1.2).contains(&self.temperature) {
            return Err("Temperature must be between 0.1 and 1.2.".to_string());
        }
        if !(0.1..=1.0).contains(&self.top_p) {
            return Err("Probability Threshold must be between 0.1 and 1.0.".to_string());
        }
        let top_k = parse_in_range("Top-k Candidates", &self.top_k, 1, MAX_TOP_K)?;

        Ok(GenerationSettings {
            instruments: self.selected_instruments(),
            drum_kit: self.drum_kit.clone(),
            bpm,
            time_signature: self.time_signature.clone(),
            key_signature: self.key_signature.clone(),
            bars,
            events_per_bar,
            context_window,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k,
            allow_control_changes: self.allow_cc,
        })
    }

    pub fn request(&self) -> Result<GenerationRequest, String> {
        let seed = if self.random_seed {
            0
        } else {
            parse_number("Seed", &self.seed)?
        };
        Ok(GenerationRequest {
            settings: self.settings()?,
            batch_size: parse_in_range("Result Count", &self.batch, 1, MAX_RESULTS)?,
            seed,
            random_seed: self.random_seed,
            ..GenerationRequest::default()
        })
    }

    /// Apply a preset to the generation form.
    pub fn apply_settings(&mut self, settings: &GenerationSettings) {
        for slot in self.instruments.iter_mut() {
            *slot = false;
        }
        for name in settings.instruments.iter().take(MAX_INSTRUMENTS) {
            if let Some(index) = crate::core::midi::gm::patch_number(name) {
                self.instruments[index as usize] = true;
            }
        }
        self.drum_kit = settings.drum_kit.clone();
        self.time_signature = settings.time_signature.clone();
        self.key_signature = settings.key_signature.clone();
        self.bpm = settings.bpm.to_string();
        self.bars = settings.bars.to_string();
        self.events_per_bar = settings.events_per_bar.to_string();
        self.context_window = if settings.context_window == 0 {
            AUTO_VALUE.to_string()
        } else {
            settings.context_window.to_string()
        };
        self.temperature = settings.temperature;
        self.top_p = settings.top_p;
        self.top_k = settings.top_k.to_string();
        self.allow_cc = settings.allow_control_changes;
    }

    /// Build the target length and event-budget summary.
    pub fn length_label(&self) -> String {
        let Ok((bpm, bars, events_per_bar)) = self.composition_dimensions() else {
            return "Enter valid tempo, length, and event budget values.".to_string();
        };
        let settings = GenerationSettings {
            bpm,
            bars,
            events_per_bar,
            time_signature: self.time_signature.clone(),
            ..GenerationSettings::default()
        };
        let seconds = settings.estimated_seconds();
        let minutes = (seconds / 60.0) as u64;
        let secs = (seconds % 60.0) as u64;
        let events = settings.event_count();
        format!("Target length: {minutes} min {secs:02} sec; base budget: {events} MIDI events")
    }

    fn composition_dimensions(&self) -> Result<(u16, u32, u32), String> {
        let bpm = parse_in_range("Tempo", &self.bpm, 1, MAX_BPM)?;
        let bars = parse_in_range("Length", &self.bars, 1, u32::MAX)?;
        let events_per_bar =
            parse_in_range("Event Budget per Bar", &self.events_per_bar, 1, u32::MAX)?;
        let event_budget_is_valid = bars
            .checked_mul(events_per_bar)
            .and_then(|events| events.checked_mul(16))
            .is_some();
        if !event_budget_is_valid || bars.checked_mul(128).is_none() {
            return Err("Length and event budget are too large.".to_string());
        }
        Ok((bpm, bars, events_per_bar))
    }
}

fn parse_number<T>(label: &str, text: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    text.trim()
        .parse()
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_in_range<T>(label: &str, text: &str, min: T, max: T) -> Result<T, String>
where
    T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy,
{
    let value = parse_number(label, text)?;
    if value < min || value > max {
        return Err(format!("{label} must be between {min} and {max}."));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_validation_rejects_empty_and_out_of_range_values() {
        assert!(parse_number::<u64>("Seed", "").is_err());
        assert!(parse_in_range("Result Count", "0", 1usize, 4).is_err());
        assert!(parse_in_range("Result Count", "5", 1usize, 4).is_err());
        assert_eq!(parse_in_range("Result Count", "4", 1usize, 4), Ok(4));
    }
}
