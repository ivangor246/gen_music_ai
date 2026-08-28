//! The iced `update` reducer.

use std::sync::atomic::Ordering;

use iced::Task;

use crate::services::export_midi::save_midi;
use crate::services::export_wav::save_wav;
use crate::services::model_catalog::ModelDescriptor;
use crate::services::model_store::LocalModelState;
use crate::services::preview;
use crate::services::timeline::Timeline;

use super::message::{FormMsg, Hidden, Message};
use super::state::{ActiveModel, AudioRequest, MAX_INSTRUMENTS, ModelState, State};
use super::tasks;

/// Work started before the first message. Decoding the SoundFont takes seconds,
/// so the engine is built while the user is still setting a track up; by the
/// time anything is played it is ready.
pub fn boot(state: &mut State) -> Task<Message> {
    state.player_loading = true;
    tasks::prepare_player()
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::WindowResized(width) => state.viewport_width = width,

        Message::SelectModel(name) => {
            if state.generating || state.model.is_busy() {
                return Task::none();
            }
            let Some(model) = state.model_catalog.iter().find(|model| model.name == name) else {
                return Task::none();
            };
            state.selected_model_id = model.id.clone();
            state.app_settings.set_selected_model_id(model.id.clone());
            state.confirming_model_remove = false;
            if matches!(&state.model, ModelState::Failed(_)) {
                state.model = ModelState::Idle;
            }
            state.status = format!("Selected model \"{}\".", model.name);
        }
        Message::LoadModel => {
            if state.generating || state.model.is_busy() || state.selected_model_is_active() {
                return Task::none();
            }
            let Some(model) = state.selected_model().cloned() else {
                state.status = "No supported model is available.".to_string();
                return Task::none();
            };
            let operation_id = state.next_model_operation();
            state.confirming_model_remove = false;
            state.model_cancel.store(false, Ordering::Relaxed);
            if state.model_store.local_state(&model) == LocalModelState::Installed {
                return start_model_load(
                    state,
                    model,
                    operation_id,
                    "Loading the model into memory…",
                );
            }
            state.model = ModelState::Downloading {
                downloaded: 0,
                total: model.download_size(),
            };
            state.status = format!("Downloading {}…", model.name);
            return tasks::download_model(
                state.model_store.clone(),
                model,
                state.model_cancel.clone(),
                operation_id,
            );
        }
        Message::CancelModelDownload => {
            if matches!(&state.model, ModelState::Downloading { .. }) {
                state.model_cancel.store(true, Ordering::Relaxed);
                state.model = ModelState::Cancelling;
                state.status = "Pausing the model download…".to_string();
            }
        }
        Message::ModelDownloadProgress(operation_id, downloaded, total) => {
            if operation_id == state.model_operation_id
                && matches!(&state.model, ModelState::Downloading { .. })
            {
                state.model = ModelState::Downloading { downloaded, total };
            }
        }
        Message::ModelDownloaded(operation_id, Ok(())) => {
            if operation_id != state.model_operation_id {
                return Task::none();
            }
            let Some(model) = state.selected_model().cloned() else {
                state.model = ModelState::Failed("Selected model disappeared.".to_string());
                return Task::none();
            };
            return start_model_load(
                state,
                model,
                operation_id,
                "Download verified. Loading the model into memory…",
            );
        }
        Message::ModelDownloaded(operation_id, Err(error)) => {
            if operation_id == state.model_operation_id {
                state.status = format!("Failed to download the model: {error}");
                state.model = ModelState::Failed(error);
            }
        }
        Message::ModelDownloadCancelled(operation_id) => {
            if operation_id == state.model_operation_id {
                state.model = ModelState::Idle;
                state.status = "Model download paused; it can be resumed later.".to_string();
            }
        }
        Message::ModelLoaded(operation_id, id, Ok(Hidden(model))) => {
            if operation_id == state.model_operation_id && id == state.selected_model_id {
                state.active_model = Some(ActiveModel { id, model });
                state.model = ModelState::Idle;
                state.status = "The selected model is ready to generate.".to_string();
            }
        }
        Message::ModelLoaded(operation_id, _, Err(error)) => {
            if operation_id == state.model_operation_id {
                state.status = format!("Failed to load the model: {error}");
                state.model = ModelState::Failed(error);
            }
        }
        Message::RequestModelRemoval => {
            if !state.generating
                && !state.model.is_busy()
                && state.selected_model_state() != LocalModelState::NotInstalled
            {
                state.confirming_model_remove = true;
            }
        }
        Message::CancelModelRemoval => state.confirming_model_remove = false,
        Message::ConfirmModelRemoval => {
            if state.generating || state.model.is_busy() {
                return Task::none();
            }
            let Some(model) = state.selected_model().cloned() else {
                return Task::none();
            };
            state.confirming_model_remove = false;
            if state.selected_model_is_active() {
                state.active_model = None;
            }
            let operation_id = state.next_model_operation();
            state.model = ModelState::Removing;
            state.status = format!("Removing {}…", model.name);
            return tasks::remove_model(state.model_store.clone(), model, operation_id);
        }
        Message::ModelRemoved(operation_id, id, result) => {
            if operation_id != state.model_operation_id || id != state.selected_model_id {
                return Task::none();
            }
            match result {
                Ok(()) => {
                    state.model = ModelState::Idle;
                    state.status = "Downloaded model files removed.".to_string();
                }
                Err(error) => {
                    state.status = format!("Failed to remove model files: {error}");
                    state.model = ModelState::Failed(error);
                }
            }
        }
        Message::ToggleHalfPrecision(enabled) => {
            if state.generating || state.model.is_busy() {
                return Task::none();
            }
            state.app_settings.set_half_precision(enabled);
            // Precision is baked into the weights at load time, so an already
            // loaded model has to be dropped and read again.
            if state.selected_model_is_active()
                && let Some(model) = state.selected_model().cloned()
            {
                let operation_id = state.next_model_operation();
                return start_model_load(
                    state,
                    model,
                    operation_id,
                    "Reloading the model at the new precision…",
                );
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
        Message::PreviewInstrument(index) => {
            if state.generating || index >= crate::core::midi::gm::PATCH_NAMES.len() {
                return Task::none();
            }
            if state.preview_patch == Some(index)
                || state.pending_audio == Some(AudioRequest::Preview(index))
            {
                cancel_preview(state);
                state.status = "Preview stopped.".to_string();
                return Task::none();
            }
            return request_audio(state, AudioRequest::Preview(index));
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
            state.confirming_model_remove = false;
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

        Message::PlayerReady(result) => {
            state.player_loading = false;
            match result {
                Ok(Hidden(player)) => {
                    state.player = Some(player);
                    if let Some(request) = state.pending_audio.take() {
                        start_audio(state, request);
                    }
                }
                Err(error) => {
                    state.pending_audio = None;
                    state.status = format!("Audio is unavailable: {error}");
                }
            }
        }
        Message::Play => {
            if state.timeline.is_none() {
                return Task::none();
            }
            return request_audio(state, AudioRequest::Track);
        }
        Message::Pause => {
            let result = state.player.as_ref().map(|player| player.pause());
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
        Message::Tick => advance_playback(state),
    }
    Task::none()
}

/// Mirror the audio thread's progress into the state the view reads. The engine
/// clears its own `playing` flag once a track or a preview runs out.
fn advance_playback(state: &mut State) {
    let Some(snapshot) = state.player.as_ref().map(|player| player.snapshot()) else {
        state.playing = false;
        state.preview_patch = None;
        return;
    };
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => return reset_playback(state, error),
    };
    if state.playing {
        state.position = snapshot.position;
        state.playing = snapshot.playing;
    }
    if state.preview_patch.is_some() && !snapshot.playing {
        state.preview_patch = None;
    }
}

fn start_model_load(
    state: &mut State,
    model: ModelDescriptor,
    operation_id: u64,
    status: &str,
) -> Task<Message> {
    state.active_model = None;
    state.model = ModelState::Loading;
    state.status = status.to_string();
    tasks::load_model(
        state.model_store.clone(),
        model,
        state.app_settings.half_precision(),
        operation_id,
    )
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

/// Start `request` right away, or queue it behind the one-time engine build.
/// Track playback and previews share the single engine, so they share this path.
fn request_audio(state: &mut State, request: AudioRequest) -> Task<Message> {
    if state.player.is_some() {
        start_audio(state, request);
        return Task::none();
    }
    state.pending_audio = Some(request);
    if state.player_loading {
        return Task::none();
    }
    state.player_loading = true;
    state.status = "Preparing audio…".to_string();
    tasks::prepare_player()
}

fn start_audio(state: &mut State, request: AudioRequest) {
    match request {
        AudioRequest::Track => start_track(state),
        AudioRequest::Preview(index) => start_preview(state, index),
    }
}

fn start_track(state: &mut State) {
    let Some(player) = state.player.clone() else {
        return;
    };
    if state.position >= state.duration {
        state.position = 0.0;
    }
    let Some(timeline) = state.timeline.as_ref() else {
        return;
    };
    let result = player
        .set_track(timeline)
        .and_then(|()| player.play(state.position));
    match result {
        Ok(()) => {
            state.preview_patch = None;
            state.playing = true;
            state.status = "Playing…".to_string();
        }
        Err(error) => reset_playback(state, error),
    }
}

/// Load the audition phrase over whatever the engine held. Track playback keeps
/// its position, because `start_track` reloads the timeline before resuming.
fn start_preview(state: &mut State, index: usize) {
    let Some(player) = state.player.clone() else {
        return;
    };
    let result = player
        .set_track(&preview::timeline(index as u8))
        .and_then(|()| player.play(0.0));
    match result {
        Ok(()) => {
            state.playing = false;
            state.preview_patch = Some(index);
            state.status = format!(
                "Previewing \"{}\"…",
                crate::core::midi::gm::PATCH_NAMES[index]
            );
        }
        Err(error) => reset_playback(state, error),
    }
}

/// Silence a running or queued preview, leaving the track position alone.
fn cancel_preview(state: &mut State) {
    if matches!(state.pending_audio, Some(AudioRequest::Preview(_))) {
        state.pending_audio = None;
    }
    if state.preview_patch.take().is_some()
        && let Some(error) = state.player.as_ref().and_then(|player| player.stop().err())
    {
        reset_playback(state, error);
    }
}

fn stop_playback(state: &mut State) {
    state.pending_audio = None;
    state.preview_patch = None;
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
    state.preview_patch = None;
    state.pending_audio = None;
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

/// The engine is never built in these tests: they cover the request queue that
/// keeps a click responsive while the SoundFont is still being decoded.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_queued_and_the_engine_is_built_only_once() {
        let mut state = State::new();

        let _ = update(&mut state, Message::PreviewInstrument(24));
        assert!(state.player_loading);
        assert_eq!(state.pending_audio, Some(AudioRequest::Preview(24)));

        let _ = update(&mut state, Message::PreviewInstrument(40));
        assert!(state.player_loading);
        assert_eq!(state.pending_audio, Some(AudioRequest::Preview(40)));
    }

    #[test]
    fn a_preview_waits_for_the_engine_started_at_boot() {
        let mut state = State::new();
        let _ = boot(&mut state);

        let _ = update(&mut state, Message::PreviewInstrument(24));

        assert_eq!(state.pending_audio, Some(AudioRequest::Preview(24)));
        assert!(state.player.is_none());
    }

    #[test]
    fn clicking_a_queued_preview_again_cancels_it() {
        let mut state = State::new();

        let _ = update(&mut state, Message::PreviewInstrument(7));
        let _ = update(&mut state, Message::PreviewInstrument(7));

        assert_eq!(state.pending_audio, None);
        assert_eq!(state.preview_patch, None);
    }

    #[test]
    fn playing_the_track_takes_over_a_queued_preview() {
        let mut state = State::new();
        state.timeline = Some(Timeline::default());

        let _ = update(&mut state, Message::PreviewInstrument(7));
        let _ = update(&mut state, Message::Play);

        assert_eq!(state.pending_audio, Some(AudioRequest::Track));
    }

    #[test]
    fn a_failed_engine_build_reports_and_drops_the_queued_request() {
        let mut state = State::new();

        let _ = update(&mut state, Message::PreviewInstrument(7));
        let _ = update(
            &mut state,
            Message::PlayerReady(Err("no audio output device".to_string())),
        );

        assert!(!state.player_loading);
        assert_eq!(state.pending_audio, None);
        assert_eq!(state.preview_patch, None);
        assert!(state.status.contains("no audio output device"));
    }

    #[test]
    fn generation_blocks_previews() {
        let mut state = State::new();
        state.generating = true;

        let _ = update(&mut state, Message::PreviewInstrument(7));

        assert!(!state.player_loading);
        assert_eq!(state.pending_audio, None);
    }

    #[test]
    fn a_tick_without_an_engine_clears_playback_state() {
        let mut state = State::new();
        state.preview_patch = Some(3);
        state.playing = true;

        let _ = update(&mut state, Message::Tick);

        assert_eq!(state.preview_patch, None);
        assert!(!state.playing);
    }
}
