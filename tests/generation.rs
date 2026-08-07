//! End-to-end headless generation: the pipeline produces a valid token stream.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use candle_core::Device;

use gen_music_ai::assets;
use gen_music_ai::core::model::config::ModelConfig;
use gen_music_ai::core::model::midi_model::MidiModel;
use gen_music_ai::core::tokenizer::codec::tokens_to_event;
use gen_music_ai::core::tokenizer::events::EventType;
use gen_music_ai::core::tokenizer::vocab::MAX_TOKEN_SEQ;
use gen_music_ai::services::generation::generate;
use gen_music_ai::settings::{GenerationRequest, GenerationSettings};

#[test]
fn generates_valid_tokens() {
    let device = Device::Cpu;
    let config = ModelConfig::from_json(assets::CONFIG_JSON).unwrap();
    let model = MidiModel::load(config, device, gen_music_ai::runtime::weight_dtype()).unwrap();

    let dir = std::env::temp_dir().join(format!("midi_gen_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let request = GenerationRequest {
        settings: GenerationSettings {
            bars: 1,
            instruments: vec!["Violin".to_string()],
            ..GenerationSettings::default()
        },
        batch_size: 1,
        random_seed: false,
        seed: 42,
    };

    // Cancel after a bounded number of events to keep the test fast on CPU.
    let cancel = AtomicBool::new(false);
    let events = AtomicUsize::new(0);
    let output = generate(&model, &request, &dir, &cancel, |_current, _total| {
        if events.fetch_add(1, Ordering::Relaxed) >= 12 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();

    assert_eq!(output.tracks.len(), 1);
    let path = &output.tracks[0].token_path;
    let bytes = std::fs::read(path).unwrap();
    assert!(!bytes.is_empty(), "token file should not be empty");
    assert_eq!(bytes.len() % (MAX_TOKEN_SEQ * 2), 0, "row-aligned");

    // Every row decodes (or is a structural bos/eos/pad row), and we produced notes.
    let mut saw_note = false;
    for chunk in bytes.chunks_exact(MAX_TOKEN_SEQ * 2) {
        let row: Vec<i16> = chunk
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        if let Some(event) = tokens_to_event(&row)
            && event.kind == EventType::Note
        {
            saw_note = true;
        }
    }
    assert!(saw_note, "generation should yield at least one note");

    std::fs::remove_dir_all(&dir).ok();
}
