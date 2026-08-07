//! Headless CPU generation with sliding-context sections, constrained per-slot
//! sampling, stop conditions, and cancellation.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use candle_core::{Device, Tensor};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::core::constraints::DecodeFlags;
use crate::core::midi::gm;
use crate::core::model::kv_cache::StackCache;
use crate::core::model::midi_model::MidiModel;
use crate::core::sampler::{
    SamplingParams, TokenHistory, no_repeat_ngram_bans, sample_constrained,
};
use crate::core::tokenizer::codec::{Event, TokenRow, bos_row, event_to_tokens};
use crate::core::tokenizer::events::EventType;
use crate::core::tokenizer::vocab::{
    BOS_ID, EOS_ID, MAX_TOKEN_SEQ, PAD_ID, VOCAB_SIZE, event_type_from_id,
};
use crate::services::token_store::TokenStore;
use crate::settings::{AUTO_VALUE, GenerationRequest, GenerationSettings};

/// Key signatures in tokenizer order (index -> sf/mi via idx/2, idx%2).
pub const KEY_SIGNATURES: [&str; 30] = [
    "C♭", "A♭m", "G♭", "E♭m", "D♭", "B♭m", "A♭", "Fm", "E♭", "Cm", "B♭", "Gm", "F", "Dm", "C",
    "Am", "G", "Em", "D", "Bm", "A", "F♯m", "E", "C♯m", "B", "G♯m", "F♯", "D♯m", "C♯", "A♯m",
];

/// Drum-kit name -> program number (-1 means "no drums").
pub fn drum_kit_program(name: &str) -> i32 {
    match name {
        "Standard" => 0,
        "Room" => 8,
        "Power" => 16,
        "Electronic" => 24,
        "TR-808" => 25,
        "Jazz" => 32,
        "Brush" => 40,
        "Orchestra" => 48,
        _ => -1,
    }
}

/// EOS stays masked out until a track has covered this fraction of its target
/// length, so the model can't end far short of the requested bar count.
const EOS_MIN_PROGRESS: f64 = 0.9;
/// How many recently-sampled raw token ids feed the repetition penalty and
/// n-gram guard, per track. Bounded so it stays cheap on weak devices.
const REPETITION_WINDOW: usize = 200;
/// Repetition penalty strength (1.0 = disabled). See `sample_constrained`.
const REPETITION_PENALTY: f32 = 1.3;
/// n-gram size for the no-repeat guard: if the last `NGRAM_BAN_SIZE - 1` raw
/// ids already occurred earlier, the id that followed them there is banned.
const NGRAM_BAN_SIZE: usize = 24;

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
    let params = SamplingParams {
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

    // Create all result tracks. From-scratch prompts are identical, so the
    // tracks batch together and advance in lockstep through the section loop.
    let mut tracks: Vec<TrackGen> = Vec::with_capacity(request.batch_size);
    for index in 0..request.batch_size {
        let path = cache_dir.join(format!("track_{stamp}_{}.tokens", index + 1));
        let mut store = TokenStore::create(&path, None)?;
        store.extend(&prompt_rows)?;
        let initial_end_tick = store.end_tick();
        tracks.push(TrackGen {
            store,
            initial_end_tick,
            target_tick: initial_end_tick + target_ticks,
            max_events,
            generated: 0,
            done: false,
        });
    }

    run_batches(
        model,
        &mut tracks,
        section_size as usize,
        prompt_size as usize,
        &flags,
        &params,
        &mut rng,
        cancel,
        total_work,
        target_ticks,
        &mut progress,
    )?;

    let mut outputs = Vec::with_capacity(tracks.len());
    for track in tracks {
        let target_tick = track.target_tick;
        let token_path = track.store.finish()?;
        outputs.push(GeneratedTrack {
            token_path,
            target_tick,
        });
    }
    Ok(GenerationOutput {
        tracks: outputs,
        seed,
    })
}

struct TrackGen {
    store: TokenStore,
    initial_end_tick: i64,
    target_tick: i64,
    max_events: usize,
    generated: usize,
    done: bool,
}

impl TrackGen {
    fn has_work(&self) -> bool {
        !self.done && self.store.end_tick() < self.target_tick && self.generated < self.max_events
    }

    fn completed(&self, target_ticks: i64) -> i64 {
        (self.store.end_tick() - self.initial_end_tick).clamp(0, target_ticks)
    }
}

/// Generate all tracks in lockstep batches: each outer step runs one base-net
/// forward over the whole batch, then decodes one event per track. Tracks that
/// reach their target or emit eos drop out of the batch.
#[allow(clippy::too_many_arguments)]
fn run_batches(
    model: &MidiModel,
    tracks: &mut [TrackGen],
    section_size: usize,
    prompt_size: usize,
    flags: &DecodeFlags,
    params: &SamplingParams,
    rng: &mut ChaCha8Rng,
    cancel: &AtomicBool,
    total_work: i64,
    target_ticks: i64,
    progress: &mut impl FnMut(i64, i64),
) -> Result<()> {
    let device = model.device().clone();
    // Both caches are allocated once and rewound per section / per event, so a
    // long run never re-allocates them.
    let mut base_cache = model.base_cache(prompt_size + section_size);
    let mut token_cache = model.token_cache(MAX_TOKEN_SEQ);
    // One rolling id window per track, indexed like `tracks`.
    let mut histories: Vec<TokenHistory> = (0..tracks.len())
        .map(|_| TokenHistory::new(VOCAB_SIZE as usize, REPETITION_WINDOW))
        .collect();

    while !cancel.load(Ordering::Relaxed) {
        let active: Vec<usize> = (0..tracks.len())
            .filter(|&i| tracks[i].has_work())
            .collect();
        if active.is_empty() {
            break;
        }
        // Batch only tracks that share a prompt length (normally all active ones,
        // since the batch advances in lockstep); the rest wait for the next pass.
        let mut prompts: Vec<(usize, Vec<TokenRow>)> = Vec::with_capacity(active.len());
        for &index in &active {
            prompts.push((index, tracks[index].store.model_prompt(prompt_size)?));
        }
        let target_len = prompts[0].1.len();
        let batch: Vec<(usize, Vec<TokenRow>)> = prompts
            .into_iter()
            .filter(|(_, prompt)| prompt.len() == target_len)
            .collect();
        let batch_idx: Vec<usize> = batch.iter().map(|(index, _)| *index).collect();
        let prompt_rows: Vec<Vec<TokenRow>> = batch.into_iter().map(|(_, prompt)| prompt).collect();

        let events_in_section = batch_idx
            .iter()
            .map(|&index| tracks[index].max_events - tracks[index].generated)
            .min()
            .unwrap_or(0)
            .min(section_size);

        let mut input = stack_rows(&prompt_rows, &device)?;
        base_cache.reset();

        for _ in 0..events_in_section {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let hidden = model.base_forward(&input, &mut base_cache)?;
            let allow_eos: Vec<bool> = batch_idx
                .iter()
                .map(|&index| {
                    let track = &tracks[index];
                    let progress =
                        track.completed(target_ticks) as f64 / target_ticks.max(1) as f64;
                    progress >= EOS_MIN_PROGRESS || track.generated + 1 >= track.max_events
                })
                .collect();
            let rows = sample_event_batch(
                model,
                &hidden,
                &mut token_cache,
                flags,
                params,
                rng,
                &batch_idx,
                &allow_eos,
                &mut histories,
            )?;
            for (position, &index) in batch_idx.iter().enumerate() {
                let row = rows[position];
                if row[0] == EOS_ID as i16 {
                    tracks[index].done = true;
                } else if tracks[index].has_work() {
                    tracks[index].store.append(&row)?;
                    tracks[index].generated += 1;
                }
            }
            let completed: i64 = tracks
                .iter()
                .map(|track| track.completed(target_ticks))
                .sum();
            progress(completed, total_work);
            if !batch_idx.iter().any(|&index| tracks[index].has_work()) {
                break;
            }
            input = next_rows(&rows, &device)?;
        }
    }
    Ok(())
}

/// Decode one event per batch item, applying per-item constrained sampling.
/// `allow_eos[item]` gates whether eos is a legal id at slot 0 (see
/// `EOS_MIN_PROGRESS`); the track's id history feeds the repetition penalty and
/// the no-repeat-ngram guard, both aimed at the same failure mode: a section
/// that loops the same short phrase once it starts feeding on its own output.
/// Every sampled id is recorded straight away, so at each slot the guard bans an
/// id from the same field as the slot being decoded.
#[allow(clippy::too_many_arguments)]
fn sample_event_batch(
    model: &MidiModel,
    hidden: &Tensor,
    token_cache: &mut StackCache,
    flags: &DecodeFlags,
    params: &SamplingParams,
    rng: &mut ChaCha8Rng,
    batch_idx: &[usize],
    allow_eos: &[bool],
    histories: &mut [TokenHistory],
) -> Result<Vec<TokenRow>> {
    let device = model.device().clone();
    let batch = batch_idx.len();
    token_cache.reset();
    let mut rows = vec![[PAD_ID as i16; MAX_TOKEN_SEQ]; batch];
    let mut kinds: Vec<Option<EventType>> = vec![None; batch];
    let mut ended = vec![false; batch];
    let mut last_ids = vec![0u32; batch];

    for slot in 0..MAX_TOKEN_SEQ {
        let logits = if slot == 0 {
            model.token_logits_from_hidden(hidden, token_cache)?
        } else {
            let prev = Tensor::from_vec(last_ids.clone(), (batch, 1), &device)?;
            model.token_logits_from_id(&prev, token_cache)?
        };
        let rows_logits = logits.to_vec2::<f32>()?;
        for item in 0..batch {
            if ended[item] {
                last_ids[item] = PAD_ID;
                continue;
            }
            let history = &mut histories[batch_idx[item]];
            let mut allowed = if slot == 0 {
                Cow::Borrowed(flags.event_ids(allow_eos[item]))
            } else {
                // Slot 0 only ever draws an event id or eos, so a still-running
                // item always has a kind by now.
                let kind = kinds[item].expect("event kind set at slot 0");
                Cow::Borrowed(flags.param_ids(kind, slot))
            };
            let banned = no_repeat_ngram_bans(history.ids(), NGRAM_BAN_SIZE);
            if !banned.is_empty() {
                let filtered: Vec<u32> = allowed
                    .iter()
                    .copied()
                    .filter(|id| !banned.contains(id))
                    .collect();
                // Never ban away every option; fall back to the unfiltered set.
                if !filtered.is_empty() {
                    allowed = Cow::Owned(filtered);
                }
            }
            let sample = sample_constrained(
                &rows_logits[item],
                &allowed,
                history,
                REPETITION_PENALTY,
                params,
                rng,
            );
            if sample != PAD_ID {
                history.push(sample);
            }
            last_ids[item] = sample;
            if slot == 0 {
                if sample == EOS_ID {
                    ended[item] = true;
                    rows[item][0] = EOS_ID as i16;
                } else {
                    kinds[item] = event_type_from_id(sample);
                    rows[item][0] = sample as i16;
                }
            } else {
                rows[item][slot] = sample as i16;
            }
        }
        if slot > 0
            && (0..batch).all(|item| {
                ended[item] || kinds[item].is_none_or(|kind| slot >= kind.fields().len())
            })
        {
            break;
        }
    }
    Ok(rows)
}

fn stack_rows(prompts: &[Vec<TokenRow>], device: &Device) -> Result<Tensor> {
    let batch = prompts.len();
    let length = prompts[0].len();
    let mut flat = Vec::with_capacity(batch * length * MAX_TOKEN_SEQ);
    for prompt in prompts {
        for row in prompt {
            flat.extend(row.iter().map(|&value| value as u32));
        }
    }
    Ok(Tensor::from_vec(
        flat,
        (batch, length, MAX_TOKEN_SEQ),
        device,
    )?)
}

fn next_rows(rows: &[TokenRow], device: &Device) -> Result<Tensor> {
    let batch = rows.len();
    let mut flat = Vec::with_capacity(batch * MAX_TOKEN_SEQ);
    for row in rows {
        flat.extend(row.iter().map(|&value| value as u32));
    }
    Ok(Tensor::from_vec(flat, (batch, 1, MAX_TOKEN_SEQ), device)?)
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
    if settings.key_signature != AUTO_VALUE
        && let Some(index) = KEY_SIGNATURES
            .iter()
            .position(|&k| k == settings.key_signature)
    {
        events.push(Event::new(
            EventType::KeySignature,
            vec![0, 0, 0, (index / 2) as u16, (index % 2) as u16],
        ));
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

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
