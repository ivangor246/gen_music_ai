//! Headless generation: sliding-context sections, constrained per-slot sampling,
//! stop conditions and cancellation. Mirrors the Python `GenerationService`
//! (CPU path only; GPU/offload machinery is intentionally dropped).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use candle_core::{Device, Tensor};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::core::constraints::{DecodeFlags, allowed_event_ids, allowed_param_ids};
use crate::core::midi::gm;
use crate::core::model::midi_model::MidiModel;
use crate::core::sampler::{apply_mask, sample_top_p_k, softmax_with_temp};
use crate::core::tokenizer::codec::{Event, TokenRow, bos_row, event_to_tokens};
use crate::core::tokenizer::events::EventType;
use crate::core::tokenizer::vocab::{BOS_ID, EOS_ID, MAX_TOKEN_SEQ, PAD_ID, event_type_from_id};
use crate::settings::{AUTO_VALUE, GenerationRequest, GenerationSettings};
use crate::services::token_store::TokenStore;

/// Key signatures in tokenizer order (index -> sf/mi via idx/2, idx%2).
pub const KEY_SIGNATURES: [&str; 30] = [
    "C♭", "A♭m", "G♭", "E♭m", "D♭", "B♭m", "A♭", "Fm", "E♭", "Cm", "B♭", "Gm", "F", "Dm", "C",
    "Am", "G", "Em", "D", "Bm", "A", "F♯m", "E", "C♯m", "B", "G♯m", "F♯", "D♯m", "C♯", "A♯m",
];

/// Drum-kit name -> program number (-1 means "no drums").
pub fn drum_kit_program(name: &str) -> i32 {
    match name {
        "Стандартная" => 0,
        "Комнатная" => 8,
        "Мощная" => 16,
        "Электронная" => 24,
        "TR-808" => 25,
        "Джазовая" => 32,
        "Мягкая" => 40,
        "Оркестровая" => 48,
        _ => -1,
    }
}

struct DecodeParams {
    temperature: f32,
    top_p: f32,
    top_k: usize,
}

#[derive(Debug, Clone)]
pub struct GeneratedTrack {
    pub token_path: PathBuf,
    pub target_tick: i64,
}

#[derive(Debug, Clone, Default)]
pub struct GenerationOutput {
    pub tracks: Vec<GeneratedTrack>,
    pub seed: u64,
}

pub fn generate(
    model: &MidiModel,
    request: &GenerationRequest,
    cache_dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(i64, i64),
) -> Result<GenerationOutput> {
    let settings = &request.settings;
    let seed = if request.random_seed {
        rand::random::<u32>() as u64
    } else {
        request.seed
    };
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let context_window = resolve_context_window(settings.context_window);
    let section_size = (context_window / 4).clamp(16, 128);
    let prompt_size = context_window.saturating_sub(section_size).max(32);
    let target_ticks = settings.target_ticks();
    let max_events = (settings.event_count() * 16).max(settings.bars * 128) as usize;
    let params = DecodeParams {
        temperature: settings.temperature,
        top_p: settings.top_p,
        top_k: settings.top_k,
    };

    let (prompt_rows, disabled_channels, lock_instruments) = build_initial_prompt(settings);
    let flags = DecodeFlags::new(
        lock_instruments,
        !settings.allow_control_changes,
        true, // tempo change is always disabled by the app
        &disabled_channels,
    );

    let stamp = timestamp();
    let total_work = target_ticks * request.batch_size as i64;
    let mut completed_before = 0i64;
    let mut tracks = Vec::with_capacity(request.batch_size);

    for index in 0..request.batch_size {
        let path = cache_dir.join(format!("track_{stamp}_{}.tokens", index + 1));
        let mut store = TokenStore::create(&path, None)?;
        store.extend(&prompt_rows)?;
        let initial_end_tick = store.end_tick();
        let target_tick = initial_end_tick + target_ticks;

        generate_track(
            model,
            &mut store,
            target_tick,
            initial_end_tick,
            section_size as usize,
            prompt_size as usize,
            max_events,
            &flags,
            &params,
            &mut rng,
            cancel,
            &mut |gained| progress(completed_before + gained, total_work),
        )?;

        completed_before += (store.end_tick() - initial_end_tick).min(target_ticks);
        let token_path = store.finish()?;
        tracks.push(GeneratedTrack {
            token_path,
            target_tick,
        });

        if cancel.load(Ordering::Relaxed) {
            break;
        }
    }

    Ok(GenerationOutput { tracks, seed })
}

#[allow(clippy::too_many_arguments)]
fn generate_track(
    model: &MidiModel,
    store: &mut TokenStore,
    target_tick: i64,
    initial_end_tick: i64,
    section_size: usize,
    prompt_size: usize,
    max_events: usize,
    flags: &DecodeFlags,
    params: &DecodeParams,
    rng: &mut ChaCha8Rng,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(i64),
) -> Result<()> {
    let device = model.device().clone();
    let mut generated = 0usize;
    let target_ticks = target_tick - initial_end_tick;

    while !cancel.load(Ordering::Relaxed) {
        if store.end_tick() >= target_tick || generated >= max_events {
            break;
        }
        let mut input_rows = store.model_prompt(prompt_size)?;
        let mut base_cache = model.base_cache();
        let events_in_section = section_size.min(max_events - generated);
        let mut produced = 0usize;
        let mut ended = false;

        while produced < events_in_section {
            if cancel.load(Ordering::Relaxed)
                || store.end_tick() >= target_tick
                || generated >= max_events
            {
                break;
            }
            let ids = rows_to_tensor(&input_rows, &device)?;
            let hidden = model.base_forward(&ids, &mut base_cache)?;
            match sample_event(model, &hidden, flags, params, rng)? {
                None => {
                    ended = true;
                    break;
                }
                Some(row) => {
                    store.append(&row)?;
                    generated += 1;
                    produced += 1;
                    input_rows = vec![row];
                    on_progress((store.end_tick() - initial_end_tick).min(target_ticks));
                }
            }
        }

        if ended || produced == 0 {
            break;
        }
    }
    Ok(())
}

fn sample_event(
    model: &MidiModel,
    hidden: &Tensor,
    flags: &DecodeFlags,
    params: &DecodeParams,
    rng: &mut ChaCha8Rng,
) -> Result<Option<TokenRow>> {
    let device = model.device().clone();
    let mut token_cache = model.token_cache();
    let mut row = [PAD_ID as i16; MAX_TOKEN_SEQ];
    let mut kind: Option<EventType> = None;
    let mut last_id: u32 = 0;

    for slot in 0..MAX_TOKEN_SEQ {
        let allowed = if slot == 0 {
            allowed_event_ids(flags)
        } else {
            allowed_param_ids(kind.expect("event kind set at slot 0"), slot, flags)
        };
        let logits = if slot == 0 {
            model.token_logits_from_hidden(hidden, &mut token_cache)?
        } else {
            let prev = Tensor::from_vec(vec![last_id], (1, 1), &device)?;
            model.token_logits_from_id(&prev, &mut token_cache)?
        };
        let logits = logits.flatten_all()?.to_vec1::<f32>()?;
        let mut probs = softmax_with_temp(&logits, params.temperature);
        apply_mask(&mut probs, &allowed);
        let sample = sample_top_p_k(&probs, params.top_p, params.top_k, rng);
        last_id = sample;

        if slot == 0 {
            if sample == EOS_ID {
                return Ok(None);
            }
            kind = event_type_from_id(sample);
            row[0] = sample as i16;
        } else {
            row[slot] = sample as i16;
            if slot == kind.expect("event kind set").fields().len() {
                break;
            }
        }
    }
    Ok(Some(row))
}

/// Build the from-scratch prompt: bos, time/key signature, tempo, patch changes.
/// Returns (rows, disabled channels, lock_instruments).
fn build_initial_prompt(settings: &GenerationSettings) -> (Vec<TokenRow>, Vec<u16>, bool) {
    let mut events: Vec<Event> = Vec::new();
    let (numerator, denominator) = settings.time_signature_parts();
    let denominator_code = match denominator {
        2 => 1,
        8 => 3,
        _ => 2,
    };
    events.push(Event::new(
        EventType::TimeSignature,
        vec![0, 0, 0, (numerator - 1) as u16, denominator_code - 1],
    ));
    if settings.key_signature != AUTO_VALUE {
        if let Some(index) = KEY_SIGNATURES.iter().position(|&k| k == settings.key_signature) {
            events.push(Event::new(
                EventType::KeySignature,
                vec![0, 0, 0, (index / 2) as u16, (index % 2) as u16],
            ));
        }
    }
    events.push(Event::new(EventType::SetTempo, vec![0, 0, 0, settings.bpm]));

    let mut patches: Vec<(u16, u16)> = Vec::new();
    let mut channel: u16 = 0;
    for instrument in &settings.instruments {
        if let Some(patch) = gm::patch_number(instrument) {
            patches.push((channel, patch));
            channel = if channel != 8 { channel + 1 } else { 10 };
        }
    }
    let drums = drum_kit_program(&settings.drum_kit);
    if drums >= 0 {
        patches.push((9, drums as u16));
    }
    for (track, (midi_channel, patch)) in patches.iter().enumerate() {
        events.push(Event::new(
            EventType::PatchChange,
            vec![0, 0, (track + 1) as u16, *midi_channel, *patch],
        ));
    }

    let mut rows = vec![bos_row(BOS_ID)];
    rows.extend(events.iter().filter_map(event_to_tokens));

    let disabled = if patches.is_empty() {
        Vec::new()
    } else {
        (0..16u16)
            .filter(|c| !patches.iter().any(|(ch, _)| ch == c))
            .collect()
    };
    (rows, disabled, !settings.instruments.is_empty())
}

fn resolve_context_window(requested: u32) -> u32 {
    if requested > 0 {
        requested.clamp(128, 4096)
    } else {
        512
    }
}

fn rows_to_tensor(rows: &[TokenRow], device: &Device) -> Result<Tensor> {
    let flat: Vec<u32> = rows
        .iter()
        .flat_map(|row| row.iter().map(|&v| v as u32))
        .collect();
    Ok(Tensor::from_vec(flat, (1, rows.len(), MAX_TOKEN_SEQ), device)?)
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
