//! Decode a token-row stream into timed MIDI actions. Notes emit an immediate
//! note-on and a scheduled note-off, while retriggered notes of the same pitch
//! close the previous note first.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use crate::core::tokenizer::codec::{Event, TokenRow, tokens_to_event};
use crate::core::tokenizer::events::EventType;

pub const TICKS_PER_QUARTER: i64 = 480;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NoteOn {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        pitch: u8,
    },
    PatchChange {
        channel: u8,
        patch: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    SetTempo(u32),
    TimeSignature {
        numerator: u8,
        denominator_power: u8,
    },
    KeySignature {
        sharps_flats: i8,
        minor: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedAction {
    pub tick: i64,
    pub track: u16,
    pub action: Action,
}

pub fn bpm_to_tempo(bpm: u16) -> u32 {
    let bpm = bpm.max(1);
    (60.0 / bpm as f64 * 1_000_000.0) as u32
}

#[derive(PartialEq, Eq)]
struct PendingOff {
    tick: i64,
    order: u64,
    track: u16,
    channel: u8,
    pitch: u8,
}

impl Ord for PendingOff {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.tick, self.order, self.track, self.channel, self.pitch).cmp(&(
            other.tick,
            other.order,
            other.track,
            other.channel,
            other.pitch,
        ))
    }
}

impl PartialOrd for PendingOff {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct ActionStream<I> {
    rows: I,
    coarse_time: i64,
    last_tick: i64,
    order: u64,
    pending: BinaryHeap<Reverse<PendingOff>>,
    active: HashMap<(u16, u8, u8), u64>,
    queue: VecDeque<TimedAction>,
    rows_done: bool,
}

impl<I: Iterator<Item = TokenRow>> ActionStream<I> {
    pub fn new(rows: I) -> Self {
        Self {
            rows,
            coarse_time: 0,
            last_tick: 0,
            order: 0,
            pending: BinaryHeap::new(),
            active: HashMap::new(),
            queue: VecDeque::new(),
            rows_done: false,
        }
    }

    fn flush_due(&mut self, tick: i64) {
        while let Some(Reverse(off)) = self.pending.peek() {
            if off.tick > tick {
                break;
            }
            let off = self.pending.pop().unwrap().0;
            let key = (off.track, off.channel, off.pitch);
            if self.active.get(&key) == Some(&off.order) {
                self.active.remove(&key);
                self.queue.push_back(TimedAction {
                    tick: off.tick,
                    track: off.track,
                    action: Action::NoteOff {
                        channel: off.channel,
                        pitch: off.pitch,
                    },
                });
            }
        }
    }

    fn handle_event(&mut self, event: &Event) {
        let track = event.params[2];
        match event.kind {
            EventType::Note => {
                let (channel, pitch, velocity, duration) = (
                    event.params[3] as u8,
                    event.params[4] as u8,
                    event.params[5] as u8,
                    i64::from(event.params[6]),
                );
                let duration_ticks = (duration * TICKS_PER_QUARTER / 16).max(1);
                let key = (track, channel, pitch);
                if self.active.remove(&key).is_some() {
                    self.queue.push_back(TimedAction {
                        tick: self.last_tick,
                        track,
                        action: Action::NoteOff { channel, pitch },
                    });
                }
                self.queue.push_back(TimedAction {
                    tick: self.last_tick,
                    track,
                    action: Action::NoteOn {
                        channel,
                        pitch,
                        velocity,
                    },
                });
                self.order += 1;
                self.active.insert(key, self.order);
                self.pending.push(Reverse(PendingOff {
                    tick: self.last_tick + duration_ticks,
                    order: self.order,
                    track,
                    channel,
                    pitch,
                }));
            }
            EventType::PatchChange => self.push(
                track,
                Action::PatchChange {
                    channel: event.params[3] as u8,
                    patch: event.params[4] as u8,
                },
            ),
            EventType::ControlChange => self.push(
                track,
                Action::ControlChange {
                    channel: event.params[3] as u8,
                    controller: event.params[4] as u8,
                    value: event.params[5] as u8,
                },
            ),
            EventType::SetTempo => {
                self.push(track, Action::SetTempo(bpm_to_tempo(event.params[3])))
            }
            EventType::TimeSignature => self.push(
                track,
                Action::TimeSignature {
                    numerator: (event.params[3] + 1) as u8,
                    denominator_power: (event.params[4] + 1) as u8,
                },
            ),
            EventType::KeySignature => self.push(
                track,
                Action::KeySignature {
                    sharps_flats: event.params[3] as i8 - 7,
                    minor: event.params[4] as u8,
                },
            ),
        }
    }

    fn push(&mut self, track: u16, action: Action) {
        self.queue.push_back(TimedAction {
            tick: self.last_tick,
            track,
            action,
        });
    }
}

/// Cut a stream of actions off at `limit`, closing notes that are still
/// sounding instead of dropping their note-offs.
///
/// `ActionStream` drains pending note-offs last, in tick order, so stopping at
/// the first action past the limit throws away the offs of notes whose on was
/// already emitted -- and a note-on with no matching off hangs for good.
pub struct Truncated<I> {
    inner: I,
    limit: Option<i64>,
    /// Notes currently sounding, in the order they started, so the closing offs
    /// come out deterministically.
    active: Vec<(u16, u8, u8)>,
    closing: std::vec::IntoIter<TimedAction>,
    done: bool,
}

impl<I: Iterator<Item = TimedAction>> Truncated<I> {
    pub fn new(inner: I, limit: Option<i64>) -> Self {
        Self {
            inner,
            limit,
            active: Vec::new(),
            closing: Vec::new().into_iter(),
            done: false,
        }
    }

    fn close_active(&mut self, tick: i64) {
        let offs: Vec<TimedAction> = self
            .active
            .drain(..)
            .map(|(track, channel, pitch)| TimedAction {
                tick,
                track,
                action: Action::NoteOff { channel, pitch },
            })
            .collect();
        self.closing = offs.into_iter();
    }
}

impl<I: Iterator<Item = TimedAction>> Iterator for Truncated<I> {
    type Item = TimedAction;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(action) = self.closing.next() {
            return Some(action);
        }
        if self.done {
            return None;
        }
        let Some(limit) = self.limit else {
            return self.inner.next();
        };
        let Some(timed) = self.inner.next() else {
            self.done = true;
            return None;
        };
        if timed.tick > limit {
            self.done = true;
            self.close_active(limit);
            return self.closing.next();
        }
        match timed.action {
            Action::NoteOn { channel, pitch, .. } => {
                let key = (timed.track, channel, pitch);
                if !self.active.contains(&key) {
                    self.active.push(key);
                }
            }
            Action::NoteOff { channel, pitch } => {
                self.active
                    .retain(|entry| entry != &(timed.track, channel, pitch));
            }
            _ => {}
        }
        Some(timed)
    }
}

impl<I: Iterator<Item = TokenRow>> Iterator for ActionStream<I> {
    type Item = TimedAction;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(action) = self.queue.pop_front() {
                return Some(action);
            }
            if self.rows_done {
                let Reverse(off) = self.pending.pop()?;
                let key = (off.track, off.channel, off.pitch);
                if self.active.get(&key) == Some(&off.order) {
                    self.active.remove(&key);
                    return Some(TimedAction {
                        tick: off.tick,
                        track: off.track,
                        action: Action::NoteOff {
                            channel: off.channel,
                            pitch: off.pitch,
                        },
                    });
                }
                continue;
            }
            match self.rows.next() {
                None => {
                    self.rows_done = true;
                }
                Some(row) => {
                    if let Some(event) = tokens_to_event(&row) {
                        self.coarse_time += i64::from(event.params[0]);
                        let tick = self.last_tick.max(
                            (self.coarse_time * 16 + i64::from(event.params[1]))
                                * TICKS_PER_QUARTER
                                / 16,
                        );
                        self.flush_due(tick);
                        self.last_tick = tick;
                        self.handle_event(&event);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::codec::event_to_tokens;

    fn note(time1: u16, pitch: u16, duration: u16) -> TokenRow {
        let event = Event::new(EventType::Note, vec![time1, 0, 0, 0, pitch, 100, duration]);
        event_to_tokens(&event).unwrap()
    }

    fn actions(rows: Vec<TokenRow>, limit: Option<i64>) -> Vec<TimedAction> {
        Truncated::new(ActionStream::new(rows.into_iter()), limit).collect()
    }

    #[test]
    fn a_note_still_sounding_at_the_limit_is_closed_there() {
        // Onset at tick 480, four quarters long, so it would ring out at 2400 --
        // well past a limit of 960.
        let stream = actions(vec![note(1, 60, 64)], Some(960));
        let offs: Vec<&TimedAction> = stream
            .iter()
            .filter(|timed| matches!(timed.action, Action::NoteOff { .. }))
            .collect();
        assert_eq!(offs.len(), 1, "the note-on must get a matching off");
        assert_eq!(offs[0].tick, 960, "and it must land on the limit");
    }

    #[test]
    fn every_note_on_is_matched_even_when_truncated() {
        let rows = vec![note(0, 60, 2047), note(1, 64, 512), note(1, 67, 8)];
        let stream = actions(rows, Some(960));
        let ons = stream
            .iter()
            .filter(|timed| matches!(timed.action, Action::NoteOn { .. }))
            .count();
        let offs = stream
            .iter()
            .filter(|timed| matches!(timed.action, Action::NoteOff { .. }))
            .count();
        assert_eq!(ons, offs);
        assert!(stream.iter().all(|timed| timed.tick <= 960));
    }

    #[test]
    fn notes_that_end_before_the_limit_are_not_closed_twice() {
        // One sixteenth long at tick 0, so its off arrives at 30, far inside.
        let stream = actions(vec![note(0, 60, 1)], Some(960));
        let offs = stream
            .iter()
            .filter(|timed| matches!(timed.action, Action::NoteOff { .. }))
            .count();
        assert_eq!(offs, 1);
    }

    #[test]
    fn without_a_limit_the_stream_passes_through() {
        let rows = vec![note(0, 60, 64), note(1, 64, 16)];
        let plain: Vec<TimedAction> = ActionStream::new(rows.clone().into_iter()).collect();
        assert_eq!(actions(rows, None), plain);
    }
}
