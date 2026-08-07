//! Manual generation-speed benchmark. Run with:
//!   scripts/capped.sh cargo test --release --features heavy-tests \
//!       --test bench_gen -- --ignored --nocapture

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use candle_core::Device;

use gen_music_ai::assets;
use gen_music_ai::core::model::config::ModelConfig;
use gen_music_ai::core::model::midi_model::MidiModel;
use gen_music_ai::core::tokenizer::codec::{Event, tokens_to_event};
use gen_music_ai::services::generation::{GeneratedTrack, generate};
use gen_music_ai::services::token_store::read_rows;
use gen_music_ai::settings::{GenerationRequest, GenerationSettings};

#[test]
#[ignore]
fn bench_batched_generation() {
    let device = Device::Cpu;
    let config = ModelConfig::from_json(assets::CONFIG_JSON).unwrap();
    let load_start = Instant::now();
    let model = MidiModel::load(config, device, gen_music_ai::runtime::weight_dtype()).unwrap();
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
    };

    let steps = 20usize;
    let cancel = AtomicBool::new(false);
    let ticks = AtomicUsize::new(0);
    let start = Instant::now();
    let output = generate(&model, &request, &dir, &cancel, |_c, _t| {
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
    describe(&output.tracks[0]);

    std::fs::remove_dir_all(&dir).ok();
}

/// Print what the first track actually contains. A run that stops early does so
/// because the onset tick reached the target, so the time deltas are what to
/// look at.
fn describe(track: &GeneratedTrack) {
    let rows = read_rows(&track.token_path).unwrap();
    let events: Vec<Event> = rows.iter().filter_map(|row| tokens_to_event(row)).collect();
    let quarters: i64 = events.iter().map(|e| i64::from(e.params[0])).sum();
    let deltas: Vec<u16> = events.iter().map(|e| e.params[0]).collect();
    // Show what each event carries, so a run of identical control changes is
    // distinguishable from a legitimate channel setup sweep.
    let detail: Vec<String> = events
        .iter()
        .map(|e| match e.kind.name() {
            "note" => format!("note p{} v{} d{}", e.params[4], e.params[5], e.params[6]),
            "control_change" => format!("cc {}={}", e.params[4], e.params[5]),
            other => other.to_string(),
        })
        .collect();

    eprintln!(
        "track 1: {} events, onset {} ticks of {} target",
        events.len(),
        quarters * 480,
        track.target_tick,
    );
    eprintln!("  events: {detail:?}");
    eprintln!("  time1:  {deltas:?}");
}
