//! The iced `update` reducer.

use std::sync::atomic::Ordering;

use iced::Task;

use crate::services::export_midi::save_midi;
use crate::services::export_wav::save_wav;
use crate::services::playback::PlaybackEngine;
use crate::services::timeline::Timeline;

use super::message::{FormMsg, Hidden, Message};
use super::state::{MAX_INSTRUMENTS, ModelState, State};
use super::tasks;

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::WindowResized(width) => state.viewport_width = width,

        Message::LoadModel => {
            if matches!(&state.model, ModelState::Loading | ModelState::Ready(_)) {
                return Task::none();
            }
            state.model = ModelState::Loading;
            state.status = "Loading the model into memory…".to_string();
            return tasks::load_model(state.app_settings.half_precision());
        }
        Message::ModelLoaded(Ok(Hidden(model))) => {
            state.model = ModelState::Ready(model);
            state.status = "The model is ready to generate.".to_string();
        }
        Message::ModelLoaded(Err(error)) => {
            state.status = format!("Failed to load the model: {error}");
            state.model = ModelState::Failed(error);
        }
        Message::ToggleHalfPrecision(enabled) => {
            if state.generating || matches!(state.model, ModelState::Loading) {
                return Task::none();
            }
            state.app_settings.set_half_precision(enabled);
            // Precision is baked into the weights at load time, so an already
            // loaded model has to be dropped and read again.
            if matches!(state.model, ModelState::Ready(_)) {
                state.model = ModelState::Loading;
                state.status = "Reloading the model at the new precision…".to_string();
                return tasks::load_model(enabled);
            }
        }

        Message::Form(form) => apply_form(state, form),
        Message::InstrumentQueryInput(query) => state.instrument_query = query,
        Message::ToggleInstrument(index) => {
            let Some(selected) = state.instruments.get(index).copied() else {
                return Task::none();
            };
            if !selected && state.selected_instrument_count() >= MAX_INSTRUMENTS {
                state.status = format!("Select no more than {MAX_INSTRUMENTS} instruments.");
                return Task::none();
            }
            if let Some(slot) = state.instruments.get_mut(index) {
                *slot = !selected;
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
            let settings = match state.settings() {
                Ok(settings) => settings,
                Err(error) => {
                    state.status = format!("Cannot save preset: {error}");
                    return Task::none();
                }
            };
            let name = state.new_preset_name.clone();
            match state.preset_store.save(&name, settings) {
                Ok(()) => {
                    state.status = "Preset saved.".to_string();
                    state.new_preset_name.clear();
                }
                Err(error) => state.status = format!("{error:#}"),
            }
        }
        Message::DeletePreset => {
            if let Some(name) = state.selected_preset.clone() {
                match state.preset_store.delete(&name) {
                    Ok(()) => {
                        state.status = "Preset deleted.".to_string();
                        state.selected_preset = None;
                    }
                    Err(error) => state.status = format!("{error:#}"),
                }
            }
        }

        Message::Generate => {
            let request = match state.request() {
                Ok(request) => request,
                Err(error) => {
                    state.status = format!("Invalid settings: {error}");
                    return Task::none();
                }
            };
            let Some(model) = state.model() else {
                state.status = "Load the model first.".to_string();
                return Task::none();
            };
            if state.generating {
                return Task::none();
            }
            stop_playback(state);
            state.confirming_cache_clear = false;
            state.generating = true;
            state.progress = 0.0;
            state.status = "Generating…".to_string();
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
            state.selected_result = None;
            clear_timeline(state);
            let durations = output
                .tracks
                .iter()
                .map(track_duration)
                .collect::<Result<Vec<_>, _>>();
            let outcome = if output.cancelled {
                "Generation stopped early"
            } else {
                "Generation complete"
            };
            state.results = output.tracks;
            match durations {
                Ok(durations) => {
                    state.result_durations = durations;
                    state.status = format!("{outcome}.");
                    if !state.results.is_empty() {
                        return update(state, Message::SelectResult(0));
                    }
                }
                Err(error) => {
                    state.result_durations.clear();
                    state.status = format!("{outcome}, but results are unavailable: {error}");
                }
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
            clear_timeline(state);
            return tasks::build_timeline(index, state.results[index].clone());
        }
        Message::TimelineReady(index, result) => {
            if state.selected_result != Some(index) {
                return Task::none();
            }
            match result {
                Ok(Hidden(timeline)) => {
                    state.density = timeline.note_density(120);
                    state.duration = timeline.duration;
                    state.position = 0.0;
                    state.playing = false;
                    state.timeline = Some(timeline);
                    state.density_cache.clear();
                }
                Err(error) => {
                    clear_timeline(state);
                    state.status = error;
                }
            }
        }

        Message::SaveResultMidi(index) => return save_result(state, index, false),
        Message::SaveResultWav(index) => return save_result(state, index, true),
        Message::Saved(Ok(path)) => state.status = format!("Saved: {path}"),
        Message::Saved(Err(error)) => state.status = format!("Save failed: {error}"),
        Message::OpenSaveDirectory => {
            let directory = state.app_settings.save_directory();
            match open_path(directory) {
                Ok(()) => {
                    state.status = format!("Opened: {}", directory.display());
                }
                Err(error) => {
                    state.status = format!("Failed to open {}: {error}", directory.display());
                }
            }
        }
        Message::RequestCacheClear => {
            if !state.generating {
                state.confirming_cache_clear = true;
            }
        }
        Message::CancelCacheClear => state.confirming_cache_clear = false,
        Message::ConfirmCacheClear => {
            if !state.generating {
                clear_cache(state);
            }
        }

        Message::Play => return play(state),
        Message::Pause => {
            let result = state.player.as_ref().map(PlaybackEngine::pause);
            if let Some(Err(error)) = result {
                reset_playback(state, error);
            } else {
                state.playing = false;
            }
        }
        Message::StopPlayback => stop_playback(state),
        Message::Seek(fraction) => {
            state.position = (fraction as f64 * state.duration).clamp(0.0, state.duration);
            if state.playing {
                let result = state
                    .player
                    .as_ref()
                    .map(|player| player.play(state.position));
                if let Some(Err(error)) = result {
                    reset_playback(state, error);
                }
            }
        }
        Message::Tick => {
            if state.playing {
                let snapshot = state.player.as_ref().map(PlaybackEngine::snapshot);
                match snapshot {
                    Some(Ok(snapshot)) => {
                        state.position = snapshot.position;
                        if !snapshot.playing {
                            state.playing = false;
                        }
                    }
                    Some(Err(error)) => reset_playback(state, error),
                    None => {
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
        FormMsg::Bpm(text) => update_whole_number(&mut state.bpm, text),
        FormMsg::Bars(text) => update_whole_number(&mut state.bars, text),
        FormMsg::EventsPerBar(text) => update_whole_number(&mut state.events_per_bar, text),
        FormMsg::Temperature(value) => state.temperature = value,
        FormMsg::TopP(value) => state.top_p = value,
        FormMsg::TopK(text) => update_whole_number(&mut state.top_k, text),
        FormMsg::Batch(text) => update_whole_number(&mut state.batch, text),
        FormMsg::Seed(text) => update_whole_number(&mut state.seed, text),
        FormMsg::RandomSeed(value) => state.random_seed = value,
        FormMsg::AllowControlChanges(value) => state.allow_cc = value,
        FormMsg::DrumKit(value) => state.drum_kit = value,
        FormMsg::TimeSignature(value) => state.time_signature = value,
        FormMsg::KeySignature(value) => state.key_signature = value,
        FormMsg::ContextWindow(value) => state.context_window = value,
    }
}

fn update_whole_number(target: &mut String, value: String) {
    if value.chars().all(|character| character.is_ascii_digit()) {
        *target = value;
    }
}

fn play(state: &mut State) -> Task<Message> {
    if state.timeline.is_none() {
        return Task::none();
    }
    if state.player.is_none() {
        state.status = "Preparing audio…".to_string();
        let soundfont = match crate::assets::soundfont() {
            Ok(soundfont) => soundfont,
            Err(error) => {
                state.status = format!("Audio is unavailable: {error:#}");
                return Task::none();
            }
        };
        match PlaybackEngine::new(soundfont.as_ref()) {
            Ok(engine) => state.player = Some(engine),
            Err(error) => {
                state.status = format!("Audio is unavailable: {error}");
                return Task::none();
            }
        }
    }
    if state.position >= state.duration {
        state.position = 0.0;
    }
    if let (Some(player), Some(timeline)) = (&state.player, &state.timeline) {
        match player
            .set_track(timeline)
            .and_then(|()| player.play(state.position))
        {
            Ok(()) => {
                state.playing = true;
                state.status = "Playing…".to_string();
            }
            Err(error) => reset_playback(state, error),
        }
    }
    Task::none()
}

fn stop_playback(state: &mut State) {
    let error = state.player.as_ref().and_then(|player| player.stop().err());
    if let Some(error) = error {
        reset_playback(state, error);
    } else {
        state.playing = false;
    }
    state.position = 0.0;
    state.density_cache.clear();
}

fn reset_playback(state: &mut State, error: anyhow::Error) {
    state.player = None;
    state.playing = false;
    state.status = format!("Audio playback was reset: {error}");
}

fn clear_timeline(state: &mut State) {
    state.timeline = None;
    state.density.clear();
    state.duration = 0.0;
    state.position = 0.0;
    state.playing = false;
    state.density_cache.clear();
}

fn save_result(state: &mut State, index: usize, wav: bool) -> Task<Message> {
    let Some(track) = state.results.get(index).cloned() else {
        state.status = "The requested result is no longer available.".to_string();
        return Task::none();
    };
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
    tasks::run_once(move || {
        let result = if wav {
            crate::assets::soundfont().and_then(|soundfont| {
                save_wav(&track.token_path, &path, soundfont.as_ref(), target)
            })
        } else {
            save_midi(&track.token_path, &path, target)
        };
        match result {
            Ok(()) => Message::Saved(Ok(path.display().to_string())),
            Err(error) => Message::Saved(Err(format!("{error:#}"))),
        }
    })
}

fn clear_cache(state: &mut State) {
    let dir = crate::paths::cache_dir();
    let result = crate::services::token_store::clear_cache(&dir);
    state.confirming_cache_clear = false;
    stop_playback(state);
    state.results.clear();
    state.result_durations.clear();
    state.selected_result = None;
    clear_timeline(state);
    state.status = match result {
        Ok(removed) => format!("Service cache cleared ({removed} files removed)."),
        Err(error) => format!("Could not completely clear the service cache: {error:#}"),
    };
}

fn track_duration(track: &crate::services::generation::GeneratedTrack) -> Result<f64, String> {
    crate::services::token_store::read_rows(&track.token_path)
        .map(|rows| Timeline::build(rows.into_iter(), Some(track.target_tick)).duration)
        .map_err(|error| format!("{}: {error:#}", track.token_path.display()))
}

fn open_path(path: &std::path::Path) -> std::io::Result<()> {
    let opener = if cfg!(target_os = "windows") {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(path).spawn()?;
    Ok(())
}
