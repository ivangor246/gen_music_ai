# MIDI Track Generator

A desktop application built with **Rust** and **Iced** that generates music with a MIDI
Transformer model. It runs **entirely on the CPU** with no GPU dependencies. Inference is
powered by [candle](https://github.com/huggingface/candle), while audio synthesis is implemented
in pure Rust with [oxisynth](https://crates.io/crates/oxisynth). No web server or JavaScript is
used.

The model (`midi-model-tv2o-medium`) and instrument bank are **embedded directly into the
executable**, making the final binary self-contained with no additional downloads or setup.

## Build and Run

Rust edition 2024 and `rustc` 1.85 or newer are required. Live playback also requires the ALSA
system library (`libasound`); MIDI generation and MIDI/WAV export do not depend on it.

Run from source with repository assets for faster rebuilds:

```bash
cargo run
```

Build a self-contained binary with the embedded model:

```bash
cargo build --release --features embed
# -> target/release/gen_music_ai  (~500 MB, runnable from any directory)
```

Without the `embed` feature, the model and instrument bank are loaded from the repository's
`models/` and `assets/` directories. This is convenient during development and keeps rebuilds
from producing a new binary hundreds of megabytes in size.

## Features

- **New composition**: choose up to 15 instruments from the full GM bank, a drum kit, tempo,
  time signature, key signature, length in bars, and an event budget per bar.
- **Generation parameters**: temperature, probability threshold (top-p), number of candidates
  (top-k), result count, seed, and musical memory (context window).
- **Presets**: 19 built-in styles plus user-defined presets stored in `presets.json`.
- **Playback without saving**: play the selected result directly, seek through the timeline,
  and navigate with the interactive note-density visualization.
- **Export**: explicitly save the selected track as MIDI or WAV.

Results are generated sequentially in sections. The context size is limited by the Musical
Memory setting, so memory usage does not grow with composition length. Embedded `safetensors`
weights are loaded and converted from `bf16` to `f32` once for efficient CPU computation.

The complete token history is stored in a service cache as compact `int16` files. Music files
are not created automatically after generation: MIDI and WAV data are streamed only when the
corresponding save action is selected, without loading the entire composition into memory.
Therefore, the event count is not limited by the context window. Practical limits depend on
generation time, available disk space, and the MIDI format.

Clearing the cache removes the service history but does not delete explicitly saved MIDI or WAV
files. Cleared results can no longer be played or continued.

User data such as presets, settings, and the token cache is stored in the platform-standard XDG
data directory rather than next to the executable. This allows the binary to run from a
read-only directory.

## Project Structure

- `src/main.rs` — application entry point;
- `src/ui` — Iced interface, state, messages, panels, and note-density canvas;
- `src/services` — model loading, generation, MIDI/WAV export, synthesis, playback, token
  storage, presets, and settings;
- `src/core` — dual-Llama model, tokenizer, MIDI assembly, and synthesis;
- `src/assets.rs` — embedded model, configuration, and instrument bank;
- `tests/` — integration tests and parity checks against reference values.

The target length is determined by the number of bars, tempo, and time signature. Generation
continues until the corresponding MIDI position is reached; subsequent MIDI and WAV exports end
at the same time boundary. The event budget prevents unbounded generation if the model stops
advancing musical time.

## Tests

```bash
cargo test
```

The test suite covers numerical parity of the model's forward pass, byte-for-byte MIDI export,
timeline and note-density correctness, and end-to-end generation.
