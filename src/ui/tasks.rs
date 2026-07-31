//! Background work bridged into iced `Task`s. Heavy work runs on standard
//! threads and reports back through a channel-backed stream.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use iced::Task;
use iced::futures::{SinkExt, StreamExt};

use crate::core::model::config::ModelConfig;
use crate::core::model::midi_model::MidiModel;
use crate::services::generation::generate;
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

/// Load the model from the configured checkpoint.
pub fn load_model() -> Task<Message> {
    run_once(|| {
        let result = ModelConfig::from_json(crate::assets::CONFIG_JSON)
            .map_err(|error| error.to_string())
            .and_then(|config| {
                MidiModel::load(config, candle_core::Device::Cpu).map_err(|error| error.to_string())
            });
        Message::ModelLoaded(result.map(|model| Hidden(Arc::new(model))))
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
pub fn build_timeline(track: crate::services::generation::GeneratedTrack) -> Task<Message> {
    run_once(move || {
        let result = crate::services::token_store::read_rows(&track.token_path)
            .map(|rows| {
                crate::services::timeline::Timeline::build(
                    rows.into_iter(),
                    Some(track.target_tick),
                )
            })
            .map(Hidden);
        match result {
            Ok(timeline) => Message::TimelineReady(timeline),
            Err(_) => {
                Message::TimelineReady(Hidden(crate::services::timeline::Timeline::default()))
            }
        }
    })
}
