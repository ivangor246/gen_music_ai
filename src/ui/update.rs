//! The iced `update` reducer.

use std::sync::atomic::Ordering;

use iced::Task;

use crate::services::export_midi::save_midi;
use crate::services::export_wav::save_wav;
use crate::services::playback::PlaybackEngine;
use crate::services::timeline::Timeline;

use super::message::{FormMsg, Hidden, Message};
use super::state::{ModelState, State};
use super::tasks;

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::LoadModel => {
            if matches!(state.model, ModelState::Loading) {
                return Task::none();
            }
            state.model = ModelState::Loading;
            state.status = "Loading the model into memory…".to_string();
            return tasks::load_model();
        }
        Message::ModelLoaded(Ok(Hidden(model))) => {
            state.model = ModelState::Ready(model);
            state.status = "The model is ready to generate.".to_string();
        }
        Message::ModelLoaded(Err(error)) => {
            state.status = format!("Failed to load the model: {error}");
            state.model = ModelState::Failed(error);
        }

        Message::Form(form) => apply_form(state, form),
        Message::SelectTab(tab) => state.tab = tab,
        Message::ToggleInstrument(index) => {
            if let Some(slot) = state.instruments.get_mut(index) {
                *slot = !*slot;
            }
        }

        Message::SelectPreset(name) => {
            if let Some(preset) = state.preset_store.get(&name) {
                state.apply_settings(&preset.settings);
                state.status = format!("Applied preset \"{name}\".");
            }
            state.selected_preset = Some(name);
        }
        Message::PresetNameInput(text) => state.new_preset_name = text,
        Message::SavePreset => {
            let settings = state.settings();
            let name = state.new_preset_name.clone();
            match state.preset_store.save(&name, settings) {
                Ok(()) => {
                    state.status = "Preset saved.".to_string();
                    state.new_preset_name.clear();
                }
                Err(error) => state.status = error.to_string(),
            }
        }
        Message::DeletePreset => {
            if let Some(name) = state.selected_preset.clone() {
                match state.preset_store.delete(&name) {
                    Ok(()) => {
                        state.status = "Preset deleted.".to_string();
                        state.selected_preset = None;
                    }
                    Err(error) => state.status = error.to_string(),
                }
            }
        }

        Message::Generate => {
            let Some(model) = state.model() else {
                state.status = "Load the model first.".to_string();
                return Task::none();
            };
            if state.generating {
                return Task::none();
            }
            stop_playback(state);
            state.generating = true;
            state.progress = 0.0;
            state.status = "Generating…".to_string();
            let request = state.request();
            return tasks::generate_task(model, request, state.cancel.clone());
        }
        Message::CancelGeneration => {
            state.cancel.store(true, Ordering::Relaxed);
            state.status = "Stopping after the current event…".to_string();
        }
        Message::GenProgress(current, total) => {
            if total > 0 {
                state.progress = (current as f32 / total as f32).clamp(0.0, 1.0);
            }
        }
        Message::GenFinished(Ok(Hidden(output))) => {
            state.generating = false;
            state.progress = 1.0;
            state.seed_used = output.seed;
            state.result_durations = output
                .tracks
                .iter()
                .map(|track| track_duration(track))
                .collect();
            state.results = output.tracks;
            state.status = "Generation complete.".to_string();
            if !state.results.is_empty() {
                return update(state, Message::SelectResult(0));
            }
        }
        Message::GenFinished(Err(error)) => {
            state.generating = false;
            state.status = format!("Generation failed: {error}");
        }

        Message::SelectResult(index) => {
            if index >= state.results.len() {
                return Task::none();
            }
            stop_playback(state);
            state.selected_result = Some(index);
            state.timeline = None;
            return tasks::build_timeline(state.results[index].clone());
        }
        Message::TimelineReady(Hidden(timeline)) => {
            state.density = timeline.note_density(120);
            state.duration = timeline.duration;
            state.position = 0.0;
            state.playing = false;
            state.timeline = Some(timeline);
            state.density_cache.clear();
        }

        Message::SaveMidi => return save_selected(state, false),
        Message::SaveWav => return save_selected(state, true),
        Message::Saved(Ok(path)) => state.status = format!("Saved: {path}"),
        Message::Saved(Err(error)) => state.status = format!("Save failed: {error}"),
        Message::OpenOutputs => {
            let dir = crate::paths::outputs_dir();
            std::fs::create_dir_all(&dir).ok();
            open_path(&dir);
        }
        Message::ClearCache => clear_cache(state),

        Message::Play => return play(state),
        Message::Pause => {
            if let Some(player) = &state.player {
                player.pause();
            }
            state.playing = false;
        }
        Message::StopPlayback => stop_playback(state),
        Message::Seek(fraction) => {
            state.position = (fraction as f64 * state.duration).clamp(0.0, state.duration);
            if state.playing {
                if let Some(player) = &state.player {
                    player.play(state.position);
                }
            }
        }
        Message::Tick => {
            if state.playing {
                if let Some(player) = &state.player {
                    state.position = player.position();
                    if !player.is_playing() {
                        state.playing = false;
                    }
                }
            }
        }
    }
    Task::none()
}

fn apply_form(state: &mut State, form: FormMsg) {
    match form {
        FormMsg::Bpm(text) => state.bpm = text,
        FormMsg::Bars(text) => state.bars = text,
        FormMsg::EventsPerBar(text) => state.events_per_bar = text,
        FormMsg::Temperature(value) => state.temperature = value,
        FormMsg::TopP(value) => state.top_p = value,
        FormMsg::TopK(text) => state.top_k = text,
        FormMsg::Batch(text) => state.batch = text,
        FormMsg::Seed(text) => state.seed = text,
        FormMsg::RandomSeed(value) => state.random_seed = value,
        FormMsg::AllowControlChanges(value) => state.allow_cc = value,
        FormMsg::DrumKit(value) => state.drum_kit = value,
        FormMsg::TimeSignature(value) => state.time_signature = value,
        FormMsg::KeySignature(value) => state.key_signature = value,
        FormMsg::ContextWindow(value) => state.context_window = value,
    }
}

fn play(state: &mut State) -> Task<Message> {
    if state.timeline.is_none() {
        return Task::none();
    }
    if state.player.is_none() {
        state.status = "Preparing audio…".to_string();
        let soundfont = crate::assets::soundfont();
        match PlaybackEngine::new(soundfont.as_ref()) {
            Ok(engine) => state.player = Some(engine),
            Err(error) => {
                state.status = format!("Audio is unavailable: {error}");
                return Task::none();
            }
        }
    }
    if let (Some(player), Some(timeline)) = (&state.player, &state.timeline) {
        player.set_track(timeline);
        if state.position >= state.duration {
            state.position = 0.0;
        }
        player.play(state.position);
        state.playing = true;
        state.status = "Playing…".to_string();
    }
    Task::none()
}

fn stop_playback(state: &mut State) {
    if let Some(player) = &state.player {
        player.stop();
    }
    state.playing = false;
    state.position = 0.0;
    state.density_cache.clear();
}

fn save_selected(state: &mut State, wav: bool) -> Task<Message> {
    let Some(index) = state.selected_result else {
        state.status = "No result is selected.".to_string();
        return Task::none();
    };
    let track = state.results[index].clone();
    let extension = if wav { "wav" } else { "mid" };
    let default_name = format!("track_{}.{extension}", index + 1);
    let dialog = rfd::FileDialog::new()
        .set_directory(state.app_settings.save_directory())
        .set_file_name(&default_name)
        .add_filter(if wav { "WAV" } else { "MIDI" }, &[extension]);
    let Some(path) = dialog.save_file() else {
        return Task::none();
    };
    if let Some(parent) = path.parent() {
        state.app_settings.set_save_directory(parent.to_path_buf());
    }
    state.status = if wav {
        "Saving WAV…".to_string()
    } else {
        "Saving MIDI…".to_string()
    };

    let target = Some(track.target_tick);
    let soundfont = crate::assets::soundfont().into_owned();
    tasks::run_once(move || {
        let result = if wav {
            save_wav(&track.token_path, &path, &soundfont, target)
        } else {
            save_midi(&track.token_path, &path, target)
        };
        match result {
            Ok(()) => Message::Saved(Ok(path.display().to_string())),
            Err(error) => Message::Saved(Err(error.to_string())),
        }
    })
}

fn clear_cache(state: &mut State) {
    let dir = crate::paths::cache_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("tokens" | "json")
            ) {
                std::fs::remove_file(path).ok();
            }
        }
    }
    stop_playback(state);
    state.results.clear();
    state.result_durations.clear();
    state.selected_result = None;
    state.timeline = None;
    state.density.clear();
    state.duration = 0.0;
    state.status = "Service cache cleared.".to_string();
}

fn track_duration(track: &crate::services::generation::GeneratedTrack) -> f64 {
    crate::services::token_store::read_rows(&track.token_path)
        .map(|rows| Timeline::build(rows.into_iter(), Some(track.target_tick)).duration)
        .unwrap_or(0.0)
}

fn open_path(path: &std::path::Path) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(path).spawn().ok();
}
