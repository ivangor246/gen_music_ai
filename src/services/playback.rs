//! Real-time playback engine: a cpal output stream renders the synth in its
//! audio callback with sample-accurate MIDI scheduling. Seeking rebuilds synth
//! state (patches, control changes, and active notes) before resuming. Timing is
//! derived directly from the audio callback's sample count.
//!
//! The synth runs at the output device's own rate, so nothing resamples between
//! the two.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::core::midi::score::Action;
use crate::services::synth::{OxiSynth, SynthEngine};
use crate::services::timeline::Timeline;

const CHANNELS: cpal::ChannelCount = 2;
/// Rendering happens inside the audio callback, so the period has to leave slack
/// for a machine that is busy generating. Roughly 40 ms, clamped to the device.
const BUFFER_MILLIS: u32 = 40;

struct Scheduled {
    sample: usize,
    action: Action,
}

struct Shared {
    synth: OxiSynth,
    schedule: Vec<Scheduled>,
    cursor: usize,
    sample_pos: usize,
    total_samples: usize,
    playing: bool,
}

impl Shared {
    fn apply(&mut self, action: &Action) {
        match *action {
            Action::NoteOn {
                channel,
                pitch,
                velocity,
            } => self.synth.note_on(channel, pitch, velocity),
            Action::NoteOff { channel, pitch } => self.synth.note_off(channel, pitch),
            Action::PatchChange { channel, patch } => self.synth.program_change(channel, patch),
            Action::ControlChange {
                channel,
                controller,
                value,
            } => self.synth.control_change(channel, controller, value),
            _ => {}
        }
    }

    /// Rebuild synth state as if playback had reached `sample`, without sound:
    /// apply the last patch/CC per channel and re-trigger still-active notes.
    fn restore_state(&mut self, sample: usize) {
        self.synth.reset();
        let mut patches: HashMap<u8, u8> = HashMap::new();
        let mut controls: HashMap<(u8, u8), u8> = HashMap::new();
        let mut active: HashMap<(u8, u8), u8> = HashMap::new();
        for item in self.schedule.iter().take_while(|item| item.sample < sample) {
            match item.action {
                Action::PatchChange { channel, patch } => {
                    patches.insert(channel, patch);
                }
                Action::ControlChange {
                    channel,
                    controller,
                    value,
                } => {
                    controls.insert((channel, controller), value);
                }
                Action::NoteOn {
                    channel,
                    pitch,
                    velocity,
                } => {
                    active.insert((channel, pitch), velocity);
                }
                Action::NoteOff { channel, pitch } => {
                    active.remove(&(channel, pitch));
                }
                _ => {}
            }
        }
        for (channel, patch) in patches {
            self.synth.program_change(channel, patch);
        }
        for ((channel, controller), value) in controls {
            self.synth.control_change(channel, controller, value);
        }
        for ((channel, pitch), velocity) in active {
            self.synth.note_on(channel, pitch, velocity);
        }
        self.cursor = self
            .schedule
            .iter()
            .position(|item| item.sample >= sample)
            .unwrap_or(self.schedule.len());
        self.sample_pos = sample;
    }
}

pub struct PlaybackEngine {
    shared: Arc<Mutex<Shared>>,
    sample_rate: u32,
    _stream: cpal::Stream,
}

pub struct PlaybackSnapshot {
    pub position: f64,
    pub playing: bool,
}

impl PlaybackEngine {
    pub fn new(soundfont: &[u8]) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let supported = device
            .default_output_config()
            .map_err(|e| anyhow!("querying the audio device: {e}"))?;
        let sample_rate = supported.sample_rate();

        let synth = OxiSynth::new(soundfont, sample_rate)?;
        let shared = Arc::new(Mutex::new(Shared {
            synth,
            schedule: Vec::new(),
            cursor: 0,
            sample_pos: 0,
            total_samples: 0,
            playing: false,
        }));

        // Not every driver accepts an explicit period; falling back keeps audio
        // working rather than trading an underrun for silence.
        let requested = buffer_size(supported.buffer_size(), sample_rate);
        let stream = build_stream(&device, sample_rate, requested, shared.clone())
            .or_else(|_| {
                build_stream(
                    &device,
                    sample_rate,
                    cpal::BufferSize::Default,
                    shared.clone(),
                )
            })
            .map_err(|e| anyhow!("building audio stream: {e}"))?;
        stream.play().map_err(|e| anyhow!("starting stream: {e}"))?;

        Ok(Self {
            shared,
            sample_rate,
            _stream: stream,
        })
    }

    /// Load a timeline (in seconds) as a sample-accurate schedule.
    pub fn set_track(&self, timeline: &Timeline) -> Result<()> {
        let rate = self.sample_rate as f64;
        let mut guard = self.lock_shared()?;
        guard.schedule = timeline
            .events
            .iter()
            .map(|event| Scheduled {
                sample: (event.seconds * rate).round() as usize,
                action: event.action.clone(),
            })
            .collect();
        guard.schedule.sort_by_key(|item| item.sample);
        guard.total_samples = (timeline.duration * rate).round() as usize;
        guard.cursor = 0;
        guard.sample_pos = 0;
        guard.playing = false;
        guard.synth.reset();
        Ok(())
    }

    pub fn play(&self, position_seconds: f64) -> Result<()> {
        let sample = (position_seconds.max(0.0) * self.sample_rate as f64).round() as usize;
        let mut guard = self.lock_shared()?;
        guard.restore_state(sample);
        guard.playing = true;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        let mut guard = self.lock_shared()?;
        guard.playing = false;
        guard.synth.reset();
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        let mut guard = self.lock_shared()?;
        guard.playing = false;
        guard.sample_pos = 0;
        guard.cursor = 0;
        guard.synth.reset();
        Ok(())
    }

    pub fn snapshot(&self) -> Result<PlaybackSnapshot> {
        let guard = self.lock_shared()?;
        Ok(PlaybackSnapshot {
            position: guard.sample_pos as f64 / self.sample_rate as f64,
            playing: guard.playing,
        })
    }

    fn lock_shared(&self) -> Result<std::sync::MutexGuard<'_, Shared>> {
        self.shared
            .lock()
            .map_err(|_| anyhow!("audio playback state is unavailable"))
    }
}

fn build_stream(
    device: &cpal::Device,
    sample_rate: u32,
    buffer_size: cpal::BufferSize,
    shared: Arc<Mutex<Shared>>,
) -> Result<cpal::Stream, cpal::Error> {
    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate,
        buffer_size,
    };
    device.build_output_stream(
        config,
        move |data: &mut [f32], _| fill_audio(&shared, data),
        error_reporter(),
        None,
    )
}

fn buffer_size(supported: &cpal::SupportedBufferSize, sample_rate: u32) -> cpal::BufferSize {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            let target = sample_rate / 1_000 * BUFFER_MILLIS;
            cpal::BufferSize::Fixed(target.clamp(min, max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    }
}

/// Buffer under- and overruns recover on their own and arrive in bursts when the
/// machine is loaded, so only the first one is reported to the console.
fn error_reporter() -> impl FnMut(cpal::Error) + Send + 'static {
    let reported = AtomicBool::new(false);
    move |error| {
        if !reported.swap(true, Ordering::Relaxed) {
            eprintln!("audio stream error: {error}");
        }
    }
}

fn fill_audio(shared: &Arc<Mutex<Shared>>, data: &mut [f32]) {
    let mut guard = match shared.lock() {
        Ok(guard) => guard,
        Err(_) => {
            data.fill(0.0);
            return;
        }
    };
    if !guard.playing {
        data.fill(0.0);
        return;
    }

    let frames = data.len() / 2;
    let mut filled = 0usize;
    while filled < frames {
        // Apply all events scheduled at or before the current position.
        while guard.cursor < guard.schedule.len()
            && guard.schedule[guard.cursor].sample <= guard.sample_pos
        {
            let action = guard.schedule[guard.cursor].action.clone();
            guard.apply(&action);
            guard.cursor += 1;
        }

        if guard.sample_pos >= guard.total_samples {
            for sample in &mut data[filled * 2..] {
                *sample = 0.0;
            }
            guard.playing = false;
            return;
        }

        let next_event = guard
            .schedule
            .get(guard.cursor)
            .map(|item| item.sample)
            .unwrap_or(guard.total_samples);
        let chunk = (frames - filled)
            .min(next_event.saturating_sub(guard.sample_pos).max(1))
            .min(guard.total_samples - guard.sample_pos);
        let slice = &mut data[filled * 2..(filled + chunk) * 2];
        guard.synth.render_f32_into(slice);
        guard.sample_pos += chunk;
        filled += chunk;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_covers_the_target_latency_within_device_limits() {
        let range = cpal::SupportedBufferSize::Range {
            min: 16,
            max: 262_144,
        };
        assert_eq!(buffer_size(&range, 48_000), cpal::BufferSize::Fixed(1_920));

        // Devices that cannot reach the target keep their own ceiling.
        let tight = cpal::SupportedBufferSize::Range { min: 16, max: 256 };
        assert_eq!(buffer_size(&tight, 48_000), cpal::BufferSize::Fixed(256));

        assert_eq!(
            buffer_size(&cpal::SupportedBufferSize::Unknown, 48_000),
            cpal::BufferSize::Default
        );
    }
}
