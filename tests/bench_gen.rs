//! Manual generation-speed benchmark. Run with:
//!   cargo test --release --test bench_gen -- --ignored --nocapture

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use candle_core::Device;

use gen_music_ai::assets;
use gen_music_ai::core::model::config::ModelConfig;
use gen_music_ai::core::model::midi_model::MidiModel;
use gen_music_ai::services::generation::generate;
use gen_music_ai::settings::{GenerationRequest, GenerationSettings};

#[test]
#[ignore]
fn bench_batched_generation() {
    let device = Device::Cpu;
    let config = ModelConfig::from_json(assets::CONFIG_JSON).unwrap();
    let load_start = Instant::now();
    let model = MidiModel::load(config, device).unwrap();
    eprintln!("model load: {:.1}s", load_start.elapsed().as_secs_f64());

    let batch = 4usize;
    let dir = std::env::temp_dir().join(format!("midi_bench_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let request = GenerationRequest {
        settings: GenerationSettings {
            bars: 8,
            instruments: vec!["Acoustic Grand".to_string()],
            ..GenerationSettings::default()
        },
        batch_size: batch,
        random_seed: false,
        seed: 7,
        ..GenerationRequest::default()
    };

    let steps = 20usize;
    let cancel = AtomicBool::new(false);
    let ticks = AtomicUsize::new(0);
    let start = Instant::now();
    generate(&model, &request, &dir, &cancel, |_c, _t| {
        if ticks.fetch_add(1, Ordering::Relaxed) + 1 >= steps {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    let outer = ticks.load(Ordering::Relaxed);
    eprintln!(
        "batch={batch}: {outer} outer steps in {elapsed:.1}s = {:.2}s/step ({:.2}s per track-event)",
        elapsed / outer as f64,
        elapsed / (outer * batch) as f64,
    );

    std::fs::remove_dir_all(&dir).ok();
}
