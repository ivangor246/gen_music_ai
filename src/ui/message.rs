//! UI messages. Heavy payloads are wrapped in `Hidden` so `Message` can derive
//! `Debug` (required by iced) without those types being `Debug`.

use std::fmt;
use std::sync::Arc;

use crate::core::model::midi_model::MidiModel;
use crate::services::generation::GenerationOutput;
use crate::services::timeline::Timeline;

/// A payload that is opaque to `Debug` (iced requires `Message: Debug + Clone`).
pub struct Hidden<T>(pub T);

impl<T> fmt::Debug for Hidden<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("…")
    }
}

impl<T: Clone> Clone for Hidden<T> {
    fn clone(&self) -> Self {
        Hidden(self.0.clone())
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    WindowResized(f32),

    // Model
    LoadModel,
    ModelLoaded(Result<Hidden<Arc<MidiModel>>, String>),

    // Form
    Form(FormMsg),
    ToggleInstrument(usize),

    // Presets
    SelectPreset(String),
    SavePreset,
    DeletePreset,
    PresetNameInput(String),

    // Generation
    Generate,
    CancelGeneration,
    GenProgress(i64, i64),
    GenFinished(Result<Hidden<GenerationOutput>, String>),

    // Results / export
    SelectResult(usize),
    TimelineReady(usize, Result<Hidden<Timeline>, String>),
    SaveMidi,
    SaveWav,
    Saved(Result<String, String>),
    OpenSaveDirectory,
    RequestCacheClear,
    CancelCacheClear,
    ConfirmCacheClear,

    // Player
    Play,
    Pause,
    StopPlayback,
    Seek(f32),
    Tick,
}

#[derive(Debug, Clone)]
pub enum FormMsg {
    Bpm(String),
    Bars(String),
    EventsPerBar(String),
    Temperature(f32),
    TopP(f32),
    TopK(String),
    Batch(String),
    Seed(String),
    RandomSeed(bool),
    AllowControlChanges(bool),
    DrumKit(String),
    TimeSignature(String),
    KeySignature(String),
    ContextWindow(String),
}

/// Streaming events from a generation worker thread.
#[derive(Debug)]
pub enum GenEvent {
    Progress(i64, i64),
    Finished(Result<Hidden<GenerationOutput>, String>),
}
