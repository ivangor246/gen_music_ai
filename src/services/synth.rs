//! Pure-Rust SoundFont synthesis (oxisynth) behind a small trait so the backend
//! stays swappable. oxisynth is a fluidlite port that supports SF3 (Ogg/Vorbis)
//! soundfonts and treats channel 9 as percussion. Renders interleaved 16-bit
//! stereo.

use std::io::Cursor;

use anyhow::{Result, anyhow};
use oxisynth::{MidiEvent, SoundFont, Synth, SynthDescriptor};

pub const SAMPLE_RATE: u32 = 44_100;

pub trait SynthEngine {
    fn program_change(&mut self, channel: u8, patch: u8);
    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8);
    fn note_off(&mut self, channel: u8, pitch: u8);
    fn control_change(&mut self, channel: u8, controller: u8, value: u8);
    /// Render `frames` stereo frames, appending interleaved (L,R) i16 to `out`.
    fn render_into(&mut self, frames: usize, out: &mut Vec<i16>);
    /// Render into a pre-sized interleaved (L,R) f32 slice (len = frames * 2).
    fn render_f32_into(&mut self, out: &mut [f32]);
    fn reset(&mut self);
}

pub struct OxiSynth {
    synth: Synth,
    scratch: Vec<i16>,
}

impl OxiSynth {
    pub fn new(soundfont: &[u8], sample_rate: u32) -> Result<Self> {
        let mut synth = Synth::new(SynthDescriptor {
            sample_rate: sample_rate as f32,
            ..Default::default()
        })
        .map_err(|e| anyhow!("synth settings: {e:?}"))?;
        let mut cursor = Cursor::new(soundfont);
        let font = SoundFont::load(&mut cursor).map_err(|e| anyhow!("soundfont: {e:?}"))?;
        synth.add_font(font, true);
        Ok(Self {
            synth,
            scratch: Vec::new(),
        })
    }

    fn send(&mut self, event: MidiEvent) {
        let _ = self.synth.send_event(event);
    }
}

impl SynthEngine for OxiSynth {
    fn program_change(&mut self, channel: u8, patch: u8) {
        self.send(MidiEvent::ProgramChange {
            channel,
            program_id: patch,
        });
    }

    fn note_on(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.send(MidiEvent::NoteOn {
            channel,
            key: pitch,
            vel: velocity,
        });
    }

    fn note_off(&mut self, channel: u8, pitch: u8) {
        self.send(MidiEvent::NoteOff { channel, key: pitch });
    }

    fn control_change(&mut self, channel: u8, controller: u8, value: u8) {
        self.send(MidiEvent::ControlChange {
            channel,
            ctrl: controller,
            value,
        });
    }

    fn render_into(&mut self, frames: usize, out: &mut Vec<i16>) {
        if frames == 0 {
            return;
        }
        self.scratch.clear();
        self.scratch.resize(frames * 2, 0);
        self.synth.write(self.scratch.as_mut_slice());
        out.extend_from_slice(&self.scratch);
    }

    fn render_f32_into(&mut self, out: &mut [f32]) {
        if out.is_empty() {
            return;
        }
        self.synth.write(out);
    }

    fn reset(&mut self) {
        self.send(MidiEvent::SystemReset);
    }
}
