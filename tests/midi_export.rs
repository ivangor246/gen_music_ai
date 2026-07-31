//! Byte-for-byte parity of the Rust SMF writer against the Python reference.

use gen_music_ai::core::midi::score::ActionStream;
use gen_music_ai::core::midi::smf::write_midi;
use gen_music_ai::core::tokenizer::codec::{Event, TokenRow, event_to_tokens};
use gen_music_ai::core::tokenizer::events::EventType::*;

const TARGET_TICK: i64 = 2000;

fn rows() -> Vec<TokenRow> {
    let events = [
        Event::new(TimeSignature, vec![0, 0, 0, 3, 1]),
        Event::new(SetTempo, vec![0, 0, 0, 120]),
        Event::new(PatchChange, vec![0, 0, 1, 0, 40]),
        Event::new(Note, vec![0, 0, 1, 0, 60, 100, 16]),
        Event::new(Note, vec![1, 0, 1, 0, 64, 90, 16]),
        Event::new(Note, vec![1, 0, 1, 0, 64, 80, 8]),
        Event::new(PatchChange, vec![0, 0, 2, 1, 73]),
        Event::new(Note, vec![0, 0, 2, 1, 72, 100, 32]),
    ];
    events.iter().map(|e| event_to_tokens(e).unwrap()).collect()
}

#[test]
fn midi_bytes_match_python() {
    let stream = ActionStream::new(rows().into_iter());
    let mut produced = Vec::new();
    write_midi(stream, Some(TARGET_TICK), &mut produced).unwrap();

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/reference.mid");
    let expected = std::fs::read(path).unwrap();
    assert_eq!(produced, expected, "MIDI bytes differ from python reference");
}
