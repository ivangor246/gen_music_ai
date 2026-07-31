//! WAV export: correct PCM format and frame count co-terminating at target_tick.

use std::io::Write;

use gen_music_ai::assets;
use gen_music_ai::core::tokenizer::codec::{Event, TokenRow, event_to_tokens};
use gen_music_ai::core::tokenizer::events::EventType::*;
use gen_music_ai::services::export_wav::save_wav;

const TARGET_TICK: i64 = 2000;
// 2000 ticks at 120 BPM (500000 us/quarter) -> 2.08333s at 44100 Hz.
const EXPECTED_FRAMES: u32 = 91_875;

fn rows() -> Vec<TokenRow> {
    let events = [
        Event::new(TimeSignature, vec![0, 0, 0, 3, 1]),
        Event::new(SetTempo, vec![0, 0, 0, 120]),
        Event::new(PatchChange, vec![0, 0, 1, 0, 40]),
        Event::new(Note, vec![0, 0, 1, 0, 60, 100, 16]),
        Event::new(Note, vec![1, 0, 1, 0, 64, 90, 16]),
        Event::new(Note, vec![0, 0, 2, 1, 72, 100, 32]),
    ];
    events.iter().map(|e| event_to_tokens(e).unwrap()).collect()
}

#[test]
fn wav_has_correct_format_and_length() {
    let dir = std::env::temp_dir().join(format!("midi_wav_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let token_path = dir.join("track.tokens");
    let wav_path = dir.join("track.wav");

    let mut file = std::fs::File::create(&token_path).unwrap();
    for row in rows() {
        for value in row {
            file.write_all(&value.to_le_bytes()).unwrap();
        }
    }
    file.flush().unwrap();

    let soundfont = assets::soundfont();
    save_wav(
        &token_path,
        &wav_path,
        soundfont.as_ref(),
        Some(TARGET_TICK),
    )
    .unwrap();

    let reader = hound::WavReader::open(&wav_path).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.sample_rate, 44_100);
    assert_eq!(spec.bits_per_sample, 16);

    let frames = reader.duration();
    assert!(
        frames.abs_diff(EXPECTED_FRAMES) <= 1,
        "frames {frames} vs expected {EXPECTED_FRAMES}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
