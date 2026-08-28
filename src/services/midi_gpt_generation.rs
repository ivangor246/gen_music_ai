//! MIDI-GPT generation and conversion into the application's tv2o token store.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use candle_core::Tensor;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::core::model::midi_gpt::{
    MIDI_GPT_MAX_POSITIONS, MidiGptConfig, MidiGptModel, MidiGptToken, MidiGptVocabulary,
};
use crate::core::sampler::{SamplingParams, TokenHistory, sample_constrained};
use crate::core::tokenizer::codec::{Event, event_to_tokens};
use crate::core::tokenizer::events::EventType;
use crate::services::generation::{
    GeneratedTrack, GenerationOutput, selected_patches, setup_rows, timestamp,
};
use crate::services::token_store::TokenStore;
use crate::settings::{GenerationRequest, GenerationSettings};

const WINDOW_BARS: u32 = 4;
const MAX_CONTEXT_TOKENS: usize = 900;

#[derive(Clone, Copy)]
struct TargetTrack {
    program: u8,
    channel: u8,
    drum: bool,
}

#[derive(Clone, Copy)]
struct Note {
    track: usize,
    channel: u8,
    bar: u32,
    onset: u32,
    duration: u32,
    pitch: u8,
    velocity: u8,
}

pub fn generate(
    model: &MidiGptModel,
    request: &GenerationRequest,
    cache_dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(i64, i64),
) -> Result<GenerationOutput> {
    crate::runtime::configure_threads();
    let settings = &request.settings;
    let targets = target_tracks(settings);
    if targets.is_empty() {
        bail!("MIDI-GPT requires at least one selected instrument or drum kit");
    }
    if targets.len() > 12 {
        bail!("MIDI-GPT supports at most 12 instrument and drum tracks");
    }

    let seed = if request.random_seed {
        rand::random::<u32>() as u64
    } else {
        request.seed
    };
    let params = SamplingParams {
        temperature: settings.temperature,
        top_p: settings.top_p,
        top_k: settings.top_k,
    };
    let total = i64::from(settings.bars) * targets.len() as i64 * request.batch_size as i64;
    let stamp = timestamp();
    let mut completed = 0i64;
    let mut outputs = Vec::with_capacity(request.batch_size);

    for result_index in 0..request.batch_size {
        let mut rng = ChaCha8Rng::seed_from_u64(seed.wrapping_add(result_index as u64));
        let mut notes = Vec::new();
        for first_bar in (0..settings.bars).step_by(WINDOW_BARS as usize) {
            let mut context_tracks: Vec<Vec<u32>> = Vec::new();
            for (track_index, target) in targets.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let prompt = build_prompt(model.config(), settings, *target, &context_tracks)?;
                let (tokens, complete) =
                    generate_track(model, prompt, *target, settings, &params, &mut rng, cancel)?;
                let mut decoded =
                    decode_track(model.config(), &tokens, *target, track_index, first_bar);
                decoded.retain(|note| note.bar < settings.bars);
                notes.extend(decoded);
                context_tracks.push(tokens);
                let bars = (settings.bars - first_bar).min(WINDOW_BARS);
                completed += i64::from(if complete { bars } else { 0 });
                progress(completed, total);
                if !complete {
                    break;
                }
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }

        let path = cache_dir.join(format!("track_{stamp}_{}.tokens", result_index + 1));
        let mut store = TokenStore::create(&path, None)?;
        store.extend(&setup_rows(settings))?;
        store.extend(&notes_to_rows(notes, settings, model.config().resolution)?)?;
        let cancelled = cancel.load(Ordering::Relaxed);
        let target_tick = if cancelled {
            store.end_tick()
        } else {
            settings.target_ticks()
        };
        outputs.push(GeneratedTrack {
            token_path: store.finish()?,
            target_tick,
        });
        if cancelled {
            break;
        }
    }

    Ok(GenerationOutput {
        tracks: outputs,
        seed,
        cancelled: cancel.load(Ordering::Relaxed),
    })
}

fn target_tracks(settings: &GenerationSettings) -> Vec<TargetTrack> {
    selected_patches(settings)
        .into_iter()
        .map(|(channel, program)| TargetTrack {
            program: program as u8,
            channel: channel as u8,
            drum: channel == 9,
        })
        .collect()
}

fn build_prompt(
    config: &MidiGptConfig,
    settings: &GenerationSettings,
    target: TargetTrack,
    context_tracks: &[Vec<u32>],
) -> Result<Vec<u32>> {
    let vocabulary = &config.vocabulary;
    let piece = token(vocabulary, MidiGptToken::PieceStart, 0)?;
    let bars_value = config
        .num_bars
        .iter()
        .position(|bars| *bars == WINDOW_BARS)
        .context("MIDI-GPT does not support four-bar windows")? as u32;
    let num_bars = token(vocabulary, MidiGptToken::NumBars, bars_value)?;
    let mut context = Vec::new();
    for encoded in context_tracks.iter().rev() {
        if context.len() + encoded.len() > MAX_CONTEXT_TOKENS {
            break;
        }
        context.splice(0..0, encoded.iter().copied());
    }

    let (numerator, denominator) = settings.time_signature_parts();
    let signature = config
        .time_signature_value(numerator, denominator)
        .with_context(|| format!("MIDI-GPT does not support {numerator}/{denominator}"))?;
    let mut prompt = Vec::with_capacity(8 + context.len());
    prompt.extend([piece, num_bars]);
    prompt.extend(context);
    prompt.push(token(
        vocabulary,
        MidiGptToken::Track,
        u32::from(target.drum),
    )?);
    prompt.push(token(
        vocabulary,
        MidiGptToken::Instrument,
        config.instrument_value(target.program),
    )?);
    prompt.push(token(
        vocabulary,
        MidiGptToken::NoteDensity,
        density_level(settings.events_per_bar, target.drum),
    )?);
    prompt.push(token(vocabulary, MidiGptToken::Bar, 0)?);
    prompt.push(token(vocabulary, MidiGptToken::TimeSignature, signature)?);
    Ok(prompt)
}

#[allow(clippy::too_many_arguments)]
fn generate_track(
    model: &MidiGptModel,
    prompt: Vec<u32>,
    target: TargetTrack,
    settings: &GenerationSettings,
    params: &SamplingParams,
    rng: &mut ChaCha8Rng,
    cancel: &AtomicBool,
) -> Result<(Vec<u32>, bool)> {
    let vocabulary = &model.config().vocabulary;
    let (numerator, denominator) = settings.time_signature_parts();
    let signature = model
        .config()
        .time_signature_value(numerator, denominator)
        .context("resolving MIDI-GPT time signature")?;
    let mut grammar = Grammar::new(
        target.drum,
        signature,
        numerator,
        denominator,
        model.config().resolution,
    );
    for &id in &prompt {
        grammar.step(id, vocabulary);
    }
    let target_start = prompt
        .iter()
        .rposition(|&id| {
            vocabulary
                .decode(id)
                .is_some_and(|(kind, _)| kind == MidiGptToken::Track)
        })
        .context("MIDI-GPT prompt has no target track")?;
    let mut target_tokens = prompt[target_start..].to_vec();
    let mut cache = model.cache();
    let input = Tensor::from_vec(prompt.clone(), (1, prompt.len()), model.device())?;
    let mut logits = model
        .forward(&input, &mut cache)?
        .to_vec2::<f32>()?
        .remove(0);
    let mut history = TokenHistory::new(vocabulary.size(), 128);
    let max_new_tokens = MIDI_GPT_MAX_POSITIONS.saturating_sub(prompt.len());

    for _ in 0..max_new_tokens {
        if cancel.load(Ordering::Relaxed) {
            return Ok((target_tokens, false));
        }
        let allowed = grammar.allowed(vocabulary);
        if allowed.is_empty() {
            bail!("MIDI-GPT grammar reached a state with no legal tokens");
        }
        let sampled = sample_constrained(&logits, &allowed, &history, 1.0, params, rng);
        history.push(sampled);
        target_tokens.push(sampled);
        grammar.step(sampled, vocabulary);
        if vocabulary
            .decode(sampled)
            .is_some_and(|(kind, _)| kind == MidiGptToken::TrackEnd)
        {
            return Ok((target_tokens, true));
        }
        let input = Tensor::from_vec(vec![sampled], (1, 1), model.device())?;
        logits = model
            .forward(&input, &mut cache)?
            .to_vec2::<f32>()?
            .remove(0);
    }
    bail!(
        "MIDI-GPT exhausted its {MIDI_GPT_MAX_POSITIONS}-token context before finishing the track"
    )
}

struct Grammar {
    current: MidiGptToken,
    drum: bool,
    signature: u32,
    numerator: u32,
    denominator: u32,
    resolution: u32,
    bars: u32,
    time: i32,
    has_notes: bool,
}

impl Grammar {
    fn new(drum: bool, signature: u32, numerator: u32, denominator: u32, resolution: u32) -> Self {
        Self {
            current: MidiGptToken::PieceStart,
            drum,
            signature,
            numerator,
            denominator,
            resolution,
            bars: 0,
            time: -1,
            has_notes: false,
        }
    }

    fn step(&mut self, id: u32, vocabulary: &MidiGptVocabulary) {
        let Some((kind, value)) = vocabulary.decode(id) else {
            return;
        };
        self.current = kind;
        match kind {
            MidiGptToken::Track => {
                self.drum = value == 1;
                self.bars = 0;
            }
            MidiGptToken::Bar => {
                self.bars += 1;
                self.time = -1;
                self.has_notes = false;
            }
            MidiGptToken::TimeAbsolutePosition => self.time = value as i32,
            MidiGptToken::NoteOnset => self.has_notes = true,
            _ => {}
        }
    }

    fn allowed(&self, vocabulary: &MidiGptVocabulary) -> Vec<u32> {
        let mut ids = Vec::new();
        let mut add = |kind| ids.extend(vocabulary.ids(kind));
        match self.current {
            MidiGptToken::PieceStart => add(MidiGptToken::NumBars),
            MidiGptToken::NumBars => add(MidiGptToken::Track),
            MidiGptToken::Track => add(MidiGptToken::Instrument),
            MidiGptToken::Instrument
            | MidiGptToken::NoteDensity
            | MidiGptToken::MinPolyphony
            | MidiGptToken::MaxPolyphony
            | MidiGptToken::MinNoteDuration
            | MidiGptToken::MaxNoteDuration => add(MidiGptToken::Bar),
            MidiGptToken::Bar => {
                if let Some(id) = vocabulary.encode(MidiGptToken::TimeSignature, self.signature) {
                    ids.push(id);
                }
            }
            MidiGptToken::TimeSignature | MidiGptToken::TimeAbsolutePosition => {
                self.add_note_or_end(vocabulary, &mut ids);
            }
            MidiGptToken::VelocityLevel => add(MidiGptToken::NoteOnset),
            MidiGptToken::NoteOnset if !self.drum => add(MidiGptToken::NoteDuration),
            MidiGptToken::NoteOnset | MidiGptToken::NoteDuration => {
                self.add_note_or_end(vocabulary, &mut ids);
            }
            MidiGptToken::BarEnd if self.bars < WINDOW_BARS => add(MidiGptToken::Bar),
            MidiGptToken::BarEnd => add(MidiGptToken::TrackEnd),
            _ => {}
        }
        ids
    }

    fn add_note_or_end(&self, vocabulary: &MidiGptVocabulary, ids: &mut Vec<u32>) {
        ids.extend(vocabulary.ids(MidiGptToken::VelocityLevel));
        ids.extend(vocabulary.ids(MidiGptToken::NoteOnset));
        let bar_ticks = self.numerator * 4 * self.resolution / self.denominator;
        ids.extend(
            vocabulary
                .ids(MidiGptToken::TimeAbsolutePosition)
                .into_iter()
                .filter(|id| {
                    vocabulary
                        .decode(*id)
                        .is_some_and(|(_, value)| value as i32 > self.time && value < bar_ticks)
                }),
        );
        if self.has_notes {
            ids.extend(vocabulary.ids(MidiGptToken::BarEnd));
        }
    }
}

fn decode_track(
    config: &MidiGptConfig,
    tokens: &[u32],
    target: TargetTrack,
    track: usize,
    first_bar: u32,
) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut bar = None;
    let mut onset = 0;
    let mut velocity = 100;
    let mut pending_pitch = None;
    for &id in tokens {
        let Some((kind, value)) = config.vocabulary.decode(id) else {
            continue;
        };
        match kind {
            MidiGptToken::Bar => {
                bar = Some(bar.map_or(0, |bar| bar + 1));
                onset = 0;
                pending_pitch = None;
            }
            MidiGptToken::TimeAbsolutePosition => onset = value,
            MidiGptToken::VelocityLevel => velocity = config.velocity(value),
            MidiGptToken::NoteOnset if target.drum => {
                if let Some(bar) = bar {
                    notes.push(Note {
                        track,
                        channel: target.channel,
                        bar: first_bar + bar,
                        onset,
                        duration: 1,
                        pitch: value as u8,
                        velocity,
                    });
                }
            }
            MidiGptToken::NoteOnset => pending_pitch = Some(value as u8),
            MidiGptToken::NoteDuration => {
                if let (Some(bar), Some(pitch)) = (bar, pending_pitch.take()) {
                    notes.push(Note {
                        track,
                        channel: target.channel,
                        bar: first_bar + bar,
                        onset,
                        duration: value + 1,
                        pitch,
                        velocity,
                    });
                }
            }
            _ => {}
        }
    }
    notes
}

fn notes_to_rows(
    mut notes: Vec<Note>,
    settings: &GenerationSettings,
    resolution: u32,
) -> Result<Vec<crate::core::tokenizer::codec::TokenRow>> {
    let (numerator, denominator) = settings.time_signature_parts();
    let bar_ticks = numerator * 4 * resolution / denominator;
    notes.sort_by_key(|note| (note.bar * bar_ticks + note.onset, note.track, note.pitch));
    let mut previous_time = 0u32;
    notes
        .into_iter()
        .map(|note| {
            let absolute = internal_to_sixteenths(note.bar * bar_ticks + note.onset, resolution);
            let delta = absolute.saturating_sub(previous_time);
            previous_time = absolute;
            let duration = internal_to_sixteenths(note.duration, resolution).clamp(1, 2047);
            let event = Event::new(
                EventType::Note,
                vec![
                    (delta / 16) as u16,
                    (delta % 16) as u16,
                    (note.track + 1) as u16,
                    note.channel as u16,
                    note.pitch as u16,
                    note.velocity as u16,
                    duration as u16,
                ],
            );
            event_to_tokens(&event).context("converting a MIDI-GPT note to tv2o")
        })
        .collect()
}

fn internal_to_sixteenths(ticks: u32, resolution: u32) -> u32 {
    (ticks * 16 + resolution / 2) / resolution
}

fn density_level(events_per_bar: u32, drum: bool) -> u32 {
    let thresholds: &[u32; 9] = if drum {
        &[2, 3, 5, 8, 10, 12, 15, 18, 26]
    } else {
        &[2, 3, 4, 5, 6, 8, 10, 12, 18]
    };
    thresholds.partition_point(|threshold| events_per_bar > *threshold) as u32
}

fn token(vocabulary: &MidiGptVocabulary, kind: MidiGptToken, value: u32) -> Result<u32> {
    vocabulary
        .encode(kind, value)
        .with_context(|| format!("MIDI-GPT token {kind:?} value {value} is unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::codec::tokens_to_event;

    const YELLOW_CONFIG: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/models/midi-gpt-yellow/encoder.json"
    ));

    #[test]
    fn density_control_uses_all_quantiles() {
        assert_eq!(density_level(1, false), 0);
        assert_eq!(density_level(3, false), 1);
        assert_eq!(density_level(24, false), 9);
        assert_eq!(density_level(27, true), 9);
    }

    #[test]
    fn internal_grid_maps_to_tv2o_sixteenths() {
        assert_eq!(internal_to_sixteenths(3, 12), 4);
        assert_eq!(internal_to_sixteenths(12, 12), 16);
        assert_eq!(internal_to_sixteenths(1, 12), 1);
    }

    #[test]
    fn grammar_forces_meter_and_monotonic_time() {
        let config = MidiGptConfig::from_json(YELLOW_CONFIG).unwrap();
        let vocabulary = &config.vocabulary;
        let signature = config.time_signature_value(4, 4).unwrap();
        let mut grammar = Grammar::new(false, signature, 4, 4, config.resolution);
        for (kind, value) in [
            (MidiGptToken::PieceStart, 0),
            (MidiGptToken::NumBars, 0),
            (MidiGptToken::Track, 0),
            (MidiGptToken::Instrument, 0),
            (MidiGptToken::NoteDensity, 5),
            (MidiGptToken::Bar, 0),
        ] {
            grammar.step(vocabulary.encode(kind, value).unwrap(), vocabulary);
        }
        assert_eq!(
            grammar.allowed(vocabulary),
            vec![
                vocabulary
                    .encode(MidiGptToken::TimeSignature, signature)
                    .unwrap()
            ]
        );

        grammar.step(
            vocabulary
                .encode(MidiGptToken::TimeSignature, signature)
                .unwrap(),
            vocabulary,
        );
        grammar.step(
            vocabulary
                .encode(MidiGptToken::TimeAbsolutePosition, 5)
                .unwrap(),
            vocabulary,
        );
        let allowed = grammar.allowed(vocabulary);
        for value in 0..=5 {
            assert!(
                !allowed.contains(
                    &vocabulary
                        .encode(MidiGptToken::TimeAbsolutePosition, value)
                        .unwrap()
                )
            );
        }
        assert!(
            allowed.contains(
                &vocabulary
                    .encode(MidiGptToken::TimeAbsolutePosition, 6)
                    .unwrap()
            )
        );
        assert!(!allowed.contains(&vocabulary.encode(MidiGptToken::BarEnd, 0).unwrap()));
    }

    #[test]
    fn decoded_notes_keep_bar_and_track_timing() {
        let settings = GenerationSettings {
            time_signature: "4/4".to_string(),
            ..GenerationSettings::default()
        };
        let notes = vec![
            Note {
                track: 0,
                channel: 0,
                bar: 0,
                onset: 0,
                duration: 3,
                pitch: 60,
                velocity: 80,
            },
            Note {
                track: 1,
                channel: 1,
                bar: 1,
                onset: 0,
                duration: 12,
                pitch: 67,
                velocity: 90,
            },
        ];
        let events: Vec<Event> = notes_to_rows(notes, &settings, 12)
            .unwrap()
            .iter()
            .filter_map(|row| tokens_to_event(row))
            .collect();
        assert_eq!(events[0].params, vec![0, 0, 1, 0, 60, 80, 4]);
        assert_eq!(events[1].params, vec![4, 0, 2, 1, 67, 90, 16]);
    }
}
