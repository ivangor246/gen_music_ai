//! Timeline and note-density parity against the reference fixture.

use serde::Deserialize;

use gen_music_ai::core::midi::score::Action;
use gen_music_ai::core::tokenizer::codec::{Event, TokenRow, event_to_tokens};
use gen_music_ai::core::tokenizer::events::EventType::*;
use gen_music_ai::services::timeline::Timeline;

const TARGET_TICK: i64 = 2000;

#[derive(Deserialize)]
struct Fixture {
    duration: f64,
    density: Vec<f32>,
    note_seconds: Vec<f64>,
}

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
fn timeline_matches_reference() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/timeline.json");
    let fixture: Fixture = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

    let timeline = Timeline::build(rows().into_iter(), Some(TARGET_TICK));
    assert!(
        (timeline.duration - fixture.duration).abs() < 1e-9,
        "duration {} vs {}",
        timeline.duration,
        fixture.duration
    );

    let note_seconds: Vec<f64> = timeline
        .events
        .iter()
        .filter(|e| matches!(e.action, Action::NoteOn { .. }))
        .map(|e| e.seconds)
        .collect();
    assert_eq!(note_seconds.len(), fixture.note_seconds.len());
    for (a, b) in note_seconds.iter().zip(&fixture.note_seconds) {
        assert!((a - b).abs() < 1e-9, "note seconds {a} vs {b}");
    }

    assert_eq!(timeline.note_density(120), fixture.density);
}
