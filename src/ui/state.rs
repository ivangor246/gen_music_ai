//! Application state and the form -> settings mapping.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use iced::widget::canvas;

use crate::core::model::midi_model::MidiModel;
use crate::services::app_settings::AppSettings;
use crate::services::generation::GeneratedTrack;
use crate::services::playback::PlaybackEngine;
use crate::services::presets::PresetStore;
use crate::services::timeline::Timeline;
use crate::settings::{AUTO_VALUE, GenerationRequest, GenerationSettings};

use super::message::Tab;

pub const DRUM_KITS: [&str; 9] = [
    "Нет",
    "Стандартная",
    "Комнатная",
    "Мощная",
    "Электронная",
    "TR-808",
    "Джазовая",
    "Мягкая",
    "Оркестровая",
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
    pub tab: Tab,

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
        Self {
            model: ModelState::NotLoaded,
            status: "Готово.".to_string(),
            instruments: vec![false; 128],
            drum_kit: "Нет".to_string(),
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
            tab: Tab::NewComposition,
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

    /// Names of currently selected instruments (max 15), in program order.
    pub fn selected_instruments(&self) -> Vec<String> {
        self.instruments
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .take(15)
            .map(|(index, _)| crate::core::midi::gm::PATCH_NAMES[index].to_string())
            .collect()
    }

    pub fn settings(&self) -> GenerationSettings {
        let context_window = if self.context_window == AUTO_VALUE {
            0
        } else {
            self.context_window.parse().unwrap_or(0)
        };
        GenerationSettings {
            instruments: self.selected_instruments(),
            drum_kit: self.drum_kit.clone(),
            bpm: parse_or(&self.bpm, 120),
            time_signature: self.time_signature.clone(),
            key_signature: self.key_signature.clone(),
            bars: parse_or(&self.bars, 16),
            events_per_bar: parse_or(&self.events_per_bar, 24),
            context_window,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: parse_or(&self.top_k, 20),
            allow_control_changes: self.allow_cc,
        }
    }

    pub fn request(&self) -> GenerationRequest {
        GenerationRequest {
            settings: self.settings(),
            batch_size: parse_or::<usize>(&self.batch, 4).clamp(1, 4),
            seed: parse_or(&self.seed, 0),
            random_seed: self.random_seed,
            ..GenerationRequest::default()
        }
    }

    /// Apply a preset to the form (mirrors `_apply_preset`).
    pub fn apply_settings(&mut self, settings: &GenerationSettings) {
        for slot in self.instruments.iter_mut() {
            *slot = false;
        }
        for name in &settings.instruments {
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

    /// Target length label (mirrors `_update_length`).
    pub fn length_label(&self) -> String {
        let settings = self.settings();
        let seconds = settings.estimated_seconds();
        let minutes = (seconds / 60.0) as u64;
        let secs = (seconds % 60.0) as u64;
        let events = settings.event_count();
        format!("Целевая длина: {minutes} мин {secs:02} с; базовый резерв: {events} MIDI-событий")
    }
}

fn parse_or<T: std::str::FromStr>(text: &str, default: T) -> T {
    text.trim().parse().unwrap_or(default)
}
