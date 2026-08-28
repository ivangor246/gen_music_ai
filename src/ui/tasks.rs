//! Background work bridged into iced `Task`s. Heavy work runs on standard
//! threads and reports back through a channel-backed stream.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use iced::Task;
use iced::futures::{SinkExt, StreamExt};

use crate::core::model::midi_model::MidiModel;
use crate::services::generation::generate;
use crate::services::model_catalog::ModelDescriptor;
use crate::services::model_store::{ModelStore, was_cancelled};
use crate::services::playback::PlaybackEngine;
use crate::settings::GenerationRequest;

use super::message::{GenEvent, Hidden, Message};

/// Run a one-shot blocking job on a thread, delivering a single message.
pub fn run_once<F>(work: F) -> Task<Message>
where
    F: FnOnce() -> Message + Send + 'static,
{
    let stream = iced::stream::channel(1, move |mut output| async move {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            let _ = tx.unbounded_send(work());
        });
        if let Some(message) = rx.next().await {
            let _ = output.send(message).await;
        }
    });
    Task::stream(stream)
}

/// Build the playback engine off the UI thread: decoding the SoundFont takes
/// seconds and allocates hundreds of megabytes, so it must never block a click.
pub fn prepare_player() -> Task<Message> {
    run_once(|| {
        let result = crate::assets::soundfont()
            .and_then(|soundfont| PlaybackEngine::new(soundfont.as_ref()))
            .map(Arc::new)
            .map(Hidden)
            .map_err(|error| format!("{error:#}"));
        Message::PlayerReady(result)
    })
}

/// Load a verified installed model at the chosen precision.
pub fn load_model(
    store: ModelStore,
    model: ModelDescriptor,
    half_precision: bool,
    operation_id: u64,
) -> Task<Message> {
    run_once(move || {
        let dtype = crate::runtime::weight_dtype(half_precision);
        let id = model.id.clone();
        let result = store
            .load_bundle(&model)
            .map_err(|error| format!("{error:#}"))
            .and_then(|bundle| {
                MidiModel::load(
                    bundle.config,
                    &bundle.weights,
                    candle_core::Device::Cpu,
                    dtype,
                )
                .map_err(|error| error.to_string())
            });
        Message::ModelLoaded(
            operation_id,
            id,
            result.map(|model| Hidden(Arc::new(model))),
        )
    })
}

/// Download and verify a model without blocking the UI thread.
pub fn download_model(
    store: ModelStore,
    model: ModelDescriptor,
    cancel: Arc<AtomicBool>,
    operation_id: u64,
) -> Task<Message> {
    let stream = iced::stream::channel(32, move |mut output| async move {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            cancel.store(false, std::sync::atomic::Ordering::Relaxed);
            let progress_tx = tx.clone();
            let result = store.download(&model, &cancel, move |downloaded, total| {
                let _ = progress_tx.unbounded_send(Message::ModelDownloadProgress(
                    operation_id,
                    downloaded,
                    total,
                ));
            });
            let message = match result {
                Ok(()) => Message::ModelDownloaded(operation_id, Ok(())),
                Err(error) if was_cancelled(&error) => {
                    Message::ModelDownloadCancelled(operation_id)
                }
                Err(error) => Message::ModelDownloaded(operation_id, Err(format!("{error:#}"))),
            };
            let _ = tx.unbounded_send(message);
        });

        while let Some(message) = rx.next().await {
            let done = matches!(
                message,
                Message::ModelDownloaded(_, _) | Message::ModelDownloadCancelled(_)
            );
            let _ = output.send(message).await;
            if done {
                break;
            }
        }
    });
    Task::stream(stream)
}

pub fn remove_model(store: ModelStore, model: ModelDescriptor, operation_id: u64) -> Task<Message> {
    run_once(move || {
        let id = model.id.clone();
        let result = store.remove(&model).map_err(|error| format!("{error:#}"));
        Message::ModelRemoved(operation_id, id, result)
    })
}

/// Stream generation progress + final result from a worker thread.
pub fn generate_task(
    model: Arc<MidiModel>,
    request: GenerationRequest,
    cancel: Arc<AtomicBool>,
) -> Task<Message> {
    let stream = iced::stream::channel(64, move |mut output| async move {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        let worker = std::thread::spawn(move || {
            cancel.store(false, std::sync::atomic::Ordering::Relaxed);
            let cache = crate::paths::cache_dir();
            let progress_tx = tx.clone();
            let result = generate(&model, &request, &cache, &cancel, move |current, total| {
                let _ = progress_tx.unbounded_send(GenEvent::Progress(current, total));
            });
            let finished = match result {
                Ok(output) => GenEvent::Finished(Ok(Hidden(output))),
                Err(err) => GenEvent::Finished(Err(err.to_string())),
            };
            let _ = tx.unbounded_send(finished);
        });

        while let Some(event) = rx.next().await {
            let done = matches!(event, GenEvent::Finished(_));
            let message = match event {
                GenEvent::Progress(current, total) => Message::GenProgress(current, total),
                GenEvent::Finished(result) => Message::GenFinished(result),
            };
            let _ = output.send(message).await;
            if done {
                break;
            }
        }
        let _ = worker.join();
    });
    Task::stream(stream)
}

/// Build the playback timeline for a generated track off-thread.
pub fn build_timeline(
    index: usize,
    track: crate::services::generation::GeneratedTrack,
) -> Task<Message> {
    run_once(move || {
        let timeline = crate::services::token_store::read_rows(&track.token_path)
            .map(|rows| {
                crate::services::timeline::Timeline::build(
                    rows.into_iter(),
                    Some(track.target_tick),
                )
            })
            .map(Hidden)
            .map_err(|error| format!("Could not prepare the selected result: {error:#}"));
        Message::TimelineReady(index, timeline)
    })
}
