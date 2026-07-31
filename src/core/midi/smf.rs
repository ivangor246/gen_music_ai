//! Standard MIDI File writer, byte-compatible with the Python
//! `StreamingMidiWriter`. Consumes timed actions, buffers one byte stream per
//! track, pads every track to `target_tick`, and writes an SMF (division 480).

use std::collections::BTreeMap;
use std::io::{self, Write};

use super::score::{Action, TICKS_PER_QUARTER, TimedAction};

#[derive(Default)]
struct TrackBuffer {
    bytes: Vec<u8>,
    last_tick: i64,
}

/// Write an SMF from `actions`, ending every track at `target_tick` (if given).
pub fn write_midi(
    actions: impl Iterator<Item = TimedAction>,
    target_tick: Option<i64>,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut tracks: BTreeMap<u16, TrackBuffer> = BTreeMap::new();
    let mut global_tick = 0i64;

    for timed in actions {
        if let Some(limit) = target_tick {
            if timed.tick > limit {
                break;
            }
        }
        let tick = global_tick.max(timed.tick);
        global_tick = tick;
        if let Some(message) = message_bytes(&timed.action) {
            let track = tracks.entry(timed.track).or_default();
            write_vlq(&mut track.bytes, (tick - track.last_tick) as u64);
            track.bytes.extend_from_slice(&message);
            track.last_tick = tick;
        }
    }

    if tracks.is_empty() {
        tracks.insert(0, TrackBuffer::default());
    }
    for track in tracks.values_mut() {
        let end_tick = track.last_tick.max(target_tick.unwrap_or(track.last_tick));
        write_vlq(&mut track.bytes, (end_tick - track.last_tick) as u64);
        track.bytes.extend_from_slice(&[0xff, 0x2f, 0x00]);
    }

    let format: u16 = if tracks.len() > 1 { 1 } else { 0 };
    out.write_all(b"MThd")?;
    out.write_all(&6u32.to_be_bytes())?;
    out.write_all(&format.to_be_bytes())?;
    out.write_all(&(tracks.len() as u16).to_be_bytes())?;
    out.write_all(&(TICKS_PER_QUARTER as u16).to_be_bytes())?;
    for track in tracks.values() {
        out.write_all(b"MTrk")?;
        out.write_all(&(track.bytes.len() as u32).to_be_bytes())?;
        out.write_all(&track.bytes)?;
    }
    Ok(())
}

fn message_bytes(action: &Action) -> Option<Vec<u8>> {
    let bytes = match *action {
        Action::NoteOn {
            channel,
            pitch,
            velocity,
        } => vec![0x90 | channel, pitch, velocity],
        Action::NoteOff { channel, pitch } => vec![0x80 | channel, pitch, 0],
        Action::PatchChange { channel, patch } => vec![0xC0 | channel, patch],
        Action::ControlChange {
            channel,
            controller,
            value,
        } => vec![0xB0 | channel, controller, value],
        Action::SetTempo(tempo) => {
            let t = tempo.to_be_bytes();
            vec![0xff, 0x51, 0x03, t[1], t[2], t[3]]
        }
        Action::TimeSignature {
            numerator,
            denominator_power,
        } => vec![0xff, 0x58, 0x04, numerator, denominator_power, 24, 8],
        Action::KeySignature {
            sharps_flats,
            minor,
        } => vec![0xff, 0x59, 0x02, sharps_flats as u8, minor],
    };
    Some(bytes)
}

/// MIDI variable-length quantity (big-endian 7-bit groups).
fn write_vlq(out: &mut Vec<u8>, value: u64) {
    let mut buffer = value & 0x7f;
    let mut stack = vec![buffer as u8];
    let mut value = value;
    while value >> 7 != 0 {
        value >>= 7;
        buffer = (value & 0x7f) | 0x80;
        stack.insert(0, buffer as u8);
    }
    out.extend_from_slice(&stack);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_encodes_like_midi_spec() {
        let mut out = Vec::new();
        write_vlq(&mut out, 0);
        assert_eq!(out, vec![0x00]);
        out.clear();
        write_vlq(&mut out, 128);
        assert_eq!(out, vec![0x81, 0x00]);
        out.clear();
        write_vlq(&mut out, 0x100000);
        assert_eq!(out, vec![0xC0, 0x80, 0x00]);
    }
}
