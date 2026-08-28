//! The short phrase used to audition a General MIDI patch. It is an ordinary
//! `Timeline`, so previews run through the same playback engine as tracks.

use crate::core::midi::score::Action;
use crate::services::timeline::{Timeline, TimelineEvent};

const CHANNEL: u8 = 0;
const VELOCITY: u8 = 110;
/// A C major triad above middle C, rolled so sustained and percussive patches
/// are both recognizable.
const PITCHES: [u8; 3] = [60, 64, 67];
const ROLL_SECONDS: f64 = 0.22;
const NOTE_SECONDS: f64 = 1.0;
/// Tail kept after the last note-off so releases are not cut short.
const RELEASE_SECONDS: f64 = 0.4;

/// A one-shot timeline auditioning `patch`.
pub fn timeline(patch: u8) -> Timeline {
    let mut events = vec![TimelineEvent {
        seconds: 0.0,
        action: Action::PatchChange {
            channel: CHANNEL,
            patch,
        },
    }];
    for (index, pitch) in PITCHES.into_iter().enumerate() {
        let start = index as f64 * ROLL_SECONDS;
        events.push(TimelineEvent {
            seconds: start,
            action: Action::NoteOn {
                channel: CHANNEL,
                pitch,
                velocity: VELOCITY,
            },
        });
        events.push(TimelineEvent {
            seconds: start + NOTE_SECONDS,
            action: Action::NoteOff {
                channel: CHANNEL,
                pitch,
            },
        });
    }
    Timeline {
        events,
        duration: last_note_start() + NOTE_SECONDS + RELEASE_SECONDS,
    }
}

fn last_note_start() -> f64 {
    (PITCHES.len() - 1) as f64 * ROLL_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_plays_the_requested_patch_and_ends_after_every_note() {
        let timeline = timeline(40);

        assert_eq!(
            timeline.events[0].action,
            Action::PatchChange {
                channel: CHANNEL,
                patch: 40,
            }
        );
        let note_ons = timeline
            .events
            .iter()
            .filter(|event| matches!(event.action, Action::NoteOn { .. }))
            .count();
        assert_eq!(note_ons, PITCHES.len());
        let last_event = timeline
            .events
            .iter()
            .map(|event| event.seconds)
            .fold(0.0, f64::max);
        assert!(last_event < timeline.duration);
    }
}
