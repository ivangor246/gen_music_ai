//! Timeline in seconds for playback and the density visualization. Mirrors
//! `iter_timed_actions` / `calculate_duration` and `PlaybackTimeline`.

use crate::core::midi::score::{Action, ActionStream, TICKS_PER_QUARTER, TimedAction};
use crate::core::tokenizer::codec::TokenRow;

const DEFAULT_TEMPO: u32 = 500_000;

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub seconds: f64,
    pub action: Action,
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub events: Vec<TimelineEvent>,
    pub duration: f64,
}

impl Timeline {
    pub fn build(rows: impl Iterator<Item = TokenRow>, target_tick: Option<i64>) -> Self {
        let mut events = Vec::new();
        let mut current_tick = 0i64;
        let mut current_tempo = DEFAULT_TEMPO;
        let mut seconds = 0.0f64;

        for TimedAction { tick, action, .. } in ActionStream::new(rows) {
            if let Some(limit) = target_tick {
                if tick > limit {
                    break;
                }
            }
            let tick = current_tick.max(tick);
            seconds += ticks_to_seconds(tick - current_tick, current_tempo);
            current_tick = tick;
            if let Action::SetTempo(tempo) = action {
                events.push(TimelineEvent { seconds, action });
                current_tempo = tempo;
            } else {
                events.push(TimelineEvent { seconds, action });
            }
        }

        let mut duration = seconds;
        if let Some(limit) = target_tick {
            if limit > current_tick {
                duration += ticks_to_seconds(limit - current_tick, current_tempo);
            }
        }
        Self { events, duration }
    }

    /// Normalized note-on density over `bins` equal spans of the duration.
    pub fn note_density(&self, bins: usize) -> Vec<f32> {
        if self.duration <= 0.0 || bins == 0 {
            return vec![0.0; bins];
        }
        let mut counts = vec![0u32; bins];
        for event in &self.events {
            if matches!(event.action, Action::NoteOn { .. }) {
                let index =
                    ((event.seconds / self.duration * bins as f64) as usize).min(bins - 1);
                counts[index] += 1;
            }
        }
        let max = counts.iter().copied().max().unwrap_or(0);
        if max == 0 {
            return vec![0.0; bins];
        }
        counts.iter().map(|&c| c as f32 / max as f32).collect()
    }
}

fn ticks_to_seconds(ticks: i64, tempo: u32) -> f64 {
    ticks as f64 * tempo as f64 / TICKS_PER_QUARTER as f64 / 1_000_000.0
}
