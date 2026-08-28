//! Real-time playback engine: a cpal output stream renders the synth in its
//! audio callback with sample-accurate MIDI scheduling. Seeking rebuilds synth
//! state (patches, control changes, and active notes) before resuming. Timing is
//! derived directly from the audio callback's sample count.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::core::midi::score::Action;
use crate::services::synth::{OxiSynth, SAMPLE_RATE, SynthEngine};
use crate::services::timeline::{Timeline, TimelineEvent};

const PREVIEW_CHANNEL: u8 = 0;
const PREVIEW_PITCH: u8 = 60;
const PREVIEW_VELOCITY: u8 = 100;
const PREVIEW_NOTE_SECONDS: f64 = 0.9;
const PREVIEW_DURATION_SECONDS: f64 = 1.25;

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
    _stream: cpal::Stream,
}

pub struct PlaybackSnapshot {
    pub position: f64,
    pub playing: bool,
}

impl PlaybackEngine {
    pub fn new(soundfont: &[u8]) -> Result<Self> {
        let synth = OxiSynth::new(soundfont, SAMPLE_RATE)?;
        let shared = Arc::new(Mutex::new(Shared {
            synth,
            schedule: Vec::new(),
            cursor: 0,
            sample_pos: 0,
            total_samples: 0,
            playing: false,
        }));

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let callback_shared = shared.clone();
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| fill_audio(&callback_shared, data),
                |err| eprintln!("audio stream error: {err}"),
                None,
            )
            .map_err(|e| anyhow!("building audio stream: {e}"))?;
        stream.play().map_err(|e| anyhow!("starting stream: {e}"))?;

        Ok(Self {
            shared,
            _stream: stream,
        })
    }

    /// Load a timeline (in seconds) as a sample-accurate schedule.
    pub fn set_track(&self, timeline: &Timeline) -> Result<()> {
        let mut guard = self.lock_shared()?;
        guard.schedule = timeline
            .events
            .iter()
            .map(|event| Scheduled {
                sample: (event.seconds * SAMPLE_RATE as f64).round() as usize,
                action: event.action.clone(),
            })
            .collect();
        guard.schedule.sort_by_key(|item| item.sample);
        guard.total_samples = (timeline.duration * SAMPLE_RATE as f64).round() as usize;
        guard.cursor = 0;
        guard.sample_pos = 0;
        guard.playing = false;
        guard.synth.reset();
        Ok(())
    }

    pub fn play(&self, position_seconds: f64) -> Result<()> {
        let mut guard = self.lock_shared()?;
        let sample = (position_seconds.max(0.0) * SAMPLE_RATE as f64).round() as usize;
        guard.restore_state(sample);
        guard.playing = true;
        Ok(())
    }

    pub fn preview_patch(&self, patch: u8) -> Result<()> {
        self.set_track(&preview_timeline(patch))?;
        self.play(0.0)
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
            position: guard.sample_pos as f64 / SAMPLE_RATE as f64,
            playing: guard.playing,
        })
    }

    fn lock_shared(&self) -> Result<std::sync::MutexGuard<'_, Shared>> {
        self.shared
            .lock()
            .map_err(|_| anyhow!("audio playback state is unavailable"))
    }
}

fn preview_timeline(patch: u8) -> Timeline {
    Timeline {
        events: vec![
            TimelineEvent {
                seconds: 0.0,
                action: Action::PatchChange {
                    channel: PREVIEW_CHANNEL,
                    patch,
                },
            },
            TimelineEvent {
                seconds: 0.0,
                action: Action::NoteOn {
                    channel: PREVIEW_CHANNEL,
                    pitch: PREVIEW_PITCH,
                    velocity: PREVIEW_VELOCITY,
                },
            },
            TimelineEvent {
                seconds: PREVIEW_NOTE_SECONDS,
                action: Action::NoteOff {
                    channel: PREVIEW_CHANNEL,
                    pitch: PREVIEW_PITCH,
                },
            },
        ],
        duration: PREVIEW_DURATION_SECONDS,
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
    fn preview_uses_requested_patch_and_bounded_note() {
        let timeline = preview_timeline(40);
        assert_eq!(timeline.duration, PREVIEW_DURATION_SECONDS);
        assert_eq!(timeline.events.len(), 3);
        assert_eq!(
            timeline.events[0].action,
            Action::PatchChange {
                channel: PREVIEW_CHANNEL,
                patch: 40,
            }
        );
        assert!(matches!(timeline.events[1].action, Action::NoteOn { .. }));
        assert!(matches!(timeline.events[2].action, Action::NoteOff { .. }));
        assert!(timeline.events[2].seconds < timeline.duration);
    }
}
