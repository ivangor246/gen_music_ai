//! Application state and the form -> settings mapping.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use iced::widget::canvas;

use crate::core::model::midi_model::MidiModel;
use crate::services::app_settings::AppSettings;
use crate::services::generation::{GeneratedTrack, KEY_SIGNATURES};
use crate::services::model_catalog::{ModelCatalog, ModelDescriptor};
use crate::services::model_store::{LocalModelState, ModelStore};
use crate::services::playback::PlaybackEngine;
use crate::services::presets::PresetStore;
use crate::services::timeline::Timeline;
use crate::settings::{AUTO_VALUE, GenerationRequest, GenerationSettings};

pub const MAX_INSTRUMENTS: usize = 15;
const MAX_BPM: u16 = 383;
const MAX_BARS: u32 = 256;
const MAX_EVENTS_PER_BAR: u32 = 128;
const MAX_ESTIMATED_SECONDS: f64 = 30.0 * 60.0;
const MAX_TOTAL_EVENT_BUDGET: u64 = 8_192;
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
    Idle,
    Downloading { downloaded: u64, total: u64 },
    Cancelling,
    Loading,
    Removing,
    Failed(String),
}

impl ModelState {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Downloading { .. } | Self::Cancelling | Self::Loading | Self::Removing
        )
    }
}

pub struct ActiveModel {
    pub id: String,
    pub model: Arc<MidiModel>,
}

pub struct State {
    pub viewport_width: f32,
    pub model: ModelState,
    pub model_catalog: Vec<ModelDescriptor>,
    pub model_store: ModelStore,
    pub selected_model_id: String,
    pub active_model: Option<ActiveModel>,
    pub model_cancel: Arc<AtomicBool>,
    pub model_operation_id: u64,
    pub confirming_model_remove: bool,
    pub status: String,

    // Form
    pub instruments: Vec<bool>,
    pub instrument_query: String,
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
        let directory_status = crate::paths::ensure_runtime_directories()
            .map(|()| "Ready.".to_string())
            .unwrap_or_else(|error| format!("Could not prepare application data: {error:#}"));
        let (model_catalog, catalog_error) = match ModelCatalog::load() {
            Ok(catalog) => (catalog.models().to_vec(), None),
            Err(error) => (
                Vec::new(),
                Some(format!("Model catalog is invalid: {error:#}")),
            ),
        };
        let mut app_settings = AppSettings::load();
        let selected_model_id = app_settings
            .selected_model_id()
            .filter(|id| model_catalog.iter().any(|model| model.id == *id))
            .map(str::to_owned)
            .or_else(|| model_catalog.first().map(|model| model.id.clone()))
            .unwrap_or_default();
        if !selected_model_id.is_empty()
            && app_settings.selected_model_id() != Some(selected_model_id.as_str())
        {
            app_settings.set_selected_model_id(selected_model_id.clone());
        }
        Self {
            viewport_width: super::INITIAL_WINDOW_WIDTH,
            model: ModelState::Idle,
            model_catalog,
            model_store: ModelStore::default(),
            selected_model_id,
            active_model: None,
            model_cancel: Arc::new(AtomicBool::new(false)),
            model_operation_id: 0,
            confirming_model_remove: false,
            status: catalog_error.unwrap_or(directory_status),
            instruments: vec![false; 128],
            instrument_query: String::new(),
            drum_kit: "None".to_string(),
            time_signature: "4/4".to_string(),
            key_signature: AUTO_VALUE.to_string(),
            bpm: "120".to_string(),
            bars: "16".to_string(),
            events_per_bar: "24".to_string(),
            context_window: AUTO_VALUE.to_string(),
            temperature: 1.0,
            top_p: 0.94,
            top_k: "28".to_string(),
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
            app_settings,
        }
    }

    pub fn model(&self) -> Option<Arc<MidiModel>> {
        self.active_model
            .as_ref()
            .filter(|active| active.id == self.selected_model_id && !self.model.is_busy())
            .map(|active| active.model.clone())
    }

    pub fn selected_model(&self) -> Option<&ModelDescriptor> {
        self.model_catalog
            .iter()
            .find(|model| model.id == self.selected_model_id)
    }

    pub fn selected_model_state(&self) -> LocalModelState {
        self.selected_model()
            .map_or(LocalModelState::NotInstalled, |model| {
                self.model_store.local_state(model)
            })
    }

    pub fn selected_model_is_active(&self) -> bool {
        self.active_model
            .as_ref()
            .is_some_and(|active| active.id == self.selected_model_id)
    }

    pub fn next_model_operation(&mut self) -> u64 {
        self.model_operation_id = self.model_operation_id.wrapping_add(1);
        self.model_operation_id
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

        let settings = GenerationSettings {
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
        };
        validate_duration(&settings)?;
        Ok(settings)
    }

    pub fn request(&self) -> Result<GenerationRequest, String> {
        let seed = if self.random_seed {
            0
        } else {
            parse_number("Seed", &self.seed)?
        };
        let settings = self.settings()?;
        let batch_size = parse_in_range("Result Count", &self.batch, 1, MAX_RESULTS)?;
        validate_event_budget(&settings, batch_size)?;
        Ok(GenerationRequest {
            settings,
            batch_size,
            seed,
            random_seed: self.random_seed,
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
        let (bpm, bars, events_per_bar) = match self.composition_dimensions() {
            Ok(dimensions) => dimensions,
            Err(error) => return error,
        };
        let settings = GenerationSettings {
            bpm,
            bars,
            events_per_bar,
            time_signature: self.time_signature.clone(),
            ..GenerationSettings::default()
        };
        if let Err(error) = validate_duration(&settings) {
            return error;
        }
        let seconds = settings.estimated_seconds();
        let minutes = (seconds / 60.0) as u64;
        let secs = (seconds % 60.0) as u64;
        let events = settings.event_count();
        format!("Target length: {minutes} min {secs:02} sec; base budget: {events} MIDI events")
    }

    fn composition_dimensions(&self) -> Result<(u16, u32, u32), String> {
        let bpm = parse_in_range("Tempo", &self.bpm, 1, MAX_BPM)?;
        let bars = parse_in_range("Length", &self.bars, 1, MAX_BARS)?;
        let events_per_bar = parse_in_range(
            "Event Budget per Bar",
            &self.events_per_bar,
            1,
            MAX_EVENTS_PER_BAR,
        )?;
        Ok((bpm, bars, events_per_bar))
    }
}

fn validate_duration(settings: &GenerationSettings) -> Result<(), String> {
    if settings.estimated_seconds() > MAX_ESTIMATED_SECONDS {
        return Err("Estimated duration must not exceed 30 minutes.".to_string());
    }
    Ok(())
}

fn validate_event_budget(settings: &GenerationSettings, batch_size: usize) -> Result<(), String> {
    let total = u64::from(settings.event_count()) * batch_size as u64;
    if total > MAX_TOTAL_EVENT_BUDGET {
        return Err(format!(
            "Total event budget across all results must not exceed {MAX_TOTAL_EVENT_BUDGET}."
        ));
    }
    Ok(())
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

    #[test]
    fn workload_validation_rejects_excessive_duration_and_event_budget() {
        let long = GenerationSettings {
            bpm: 1,
            bars: 16,
            ..GenerationSettings::default()
        };
        assert!(validate_duration(&long).is_err());

        let dense = GenerationSettings {
            bars: MAX_BARS,
            events_per_bar: MAX_EVENTS_PER_BAR,
            ..GenerationSettings::default()
        };
        assert!(validate_event_budget(&dense, MAX_RESULTS).is_err());
        assert!(validate_event_budget(&GenerationSettings::default(), MAX_RESULTS).is_ok());
    }
}
