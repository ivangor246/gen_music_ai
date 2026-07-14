//! Stream a generated track to a PCM WAV, rendering the exact number of frames
//! per elapsed tick with a fractional-sample accumulator (drift-free). When a
//! target tick is given the audio ends there (co-terminating with the MIDI);
//! otherwise a one-second tail is appended. Mirrors `StreamingAudioWriter`.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::core::midi::score::{Action, ActionStream, TICKS_PER_QUARTER, TimedAction};
use crate::services::atomic::atomic_write;
use crate::services::synth::{OxiSynth, SAMPLE_RATE, SynthEngine};
use crate::services::token_store::read_rows;

const DEFAULT_TEMPO: u32 = 500_000;

pub fn save_wav(
    token_path: &Path,
    out_path: &Path,
    soundfont: &[u8],
    target_tick: Option<i64>,
) -> Result<()> {
    let rows = read_rows(token_path)?;
    let mut synth = OxiSynth::new(soundfont, SAMPLE_RATE)?;

    atomic_write(out_path, |file| {
        let spec = WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::new(&*file, spec)?;
        render(&rows, &mut synth, &mut writer, target_tick)?;
        writer.finalize()?;
        file.flush()?;
        Ok(())
    })
}

fn render<W: Write + std::io::Seek>(
    rows: &[crate::core::tokenizer::codec::TokenRow],
    synth: &mut OxiSynth,
    writer: &mut WavWriter<W>,
    target_tick: Option<i64>,
) -> Result<()> {
    let mut current_tick = 0i64;
    let mut current_tempo = DEFAULT_TEMPO;
    let mut fractional = 0.0f64;
    let mut scratch: Vec<i16> = Vec::new();

    for TimedAction { tick, action, .. } in ActionStream::new(rows.iter().copied()) {
        if let Some(limit) = target_tick {
            if tick > limit {
                break;
            }
        }
        let elapsed = (tick - current_tick).max(0);
        let exact = frames_for_ticks(elapsed, current_tempo) + fractional;
        let frames = exact as usize;
        fractional = exact - frames as f64;
        render_frames(synth, writer, frames, &mut scratch)?;
        current_tick = current_tick.max(tick);

        match action {
            Action::NoteOn {
                channel,
                pitch,
                velocity,
            } => synth.note_on(channel, pitch, velocity),
            Action::NoteOff { channel, pitch } => synth.note_off(channel, pitch),
            Action::PatchChange { channel, patch } => synth.program_change(channel, patch),
            Action::ControlChange {
                channel,
                controller,
                value,
            } => synth.control_change(channel, controller, value),
            Action::SetTempo(tempo) => current_tempo = tempo,
            _ => {}
        }
    }

    match target_tick {
        None => render_frames(synth, writer, SAMPLE_RATE as usize, &mut scratch)?,
        Some(limit) => {
            let remaining = (limit - current_tick).max(0);
            let frames = frames_for_ticks(remaining, current_tempo).round() as usize;
            render_frames(synth, writer, frames, &mut scratch)?;
        }
    }
    Ok(())
}

fn render_frames<W: Write + std::io::Seek>(
    synth: &mut OxiSynth,
    writer: &mut WavWriter<W>,
    frames: usize,
    scratch: &mut Vec<i16>,
) -> Result<()> {
    let mut remaining = frames;
    while remaining > 0 {
        let block = remaining.min(SAMPLE_RATE as usize);
        scratch.clear();
        synth.render_into(block, scratch);
        for &sample in scratch.iter() {
            writer.write_sample(sample)?;
        }
        remaining -= block;
    }
    Ok(())
}

fn frames_for_ticks(ticks: i64, tempo: u32) -> f64 {
    ticks as f64 * tempo as f64 / TICKS_PER_QUARTER as f64 / 1_000_000.0 * SAMPLE_RATE as f64
}
