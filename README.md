# MIDI Track Generator

A desktop application built with **Rust** and **Iced** that generates music with a MIDI
Transformer model. It runs **entirely on the CPU** with no GPU dependencies. Inference is
powered by [candle](https://github.com/huggingface/candle), while audio synthesis is implemented
in pure Rust with [oxisynth](https://crates.io/crates/oxisynth). No web server or JavaScript is
used.

Release builds can embed the model (`midi-model-tv2o-medium`) and instrument bank directly into
the executable, making the distributed binary self-contained.

## Build and Run

Rust edition 2024, `rustc` 1.85 or newer, `curl`, and either `sha256sum` or `shasum` are required.
Live playback also requires the ALSA system library (`libasound`); MIDI generation and MIDI/WAV
export do not depend on it.

Download the pinned runtime assets after cloning the repository. The script downloads only the
468 MB safetensors checkpoint and the 51 MB SoundFont, verifies their SHA-256 checksums, and
skips files that are already valid:

```bash
bash scripts/download-assets.sh
```

The download URLs are pinned to specific revisions of
[SkyTNT's model checkpoint](https://huggingface.co/skytnt/midi-model-tv2o-medium/tree/0f8f265d4330f4e46527ac2313200254c5757f5f)
and the [upstream SoundFont file](https://huggingface.co/skytnt/midi-model/blob/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2).

Run from source with the downloaded assets:

```bash
cargo run
```

Build a self-contained binary with the embedded model:

```bash
cargo build --release --features embed
# -> target/release/gen_music_ai  (~500 MB, runnable from any directory)
```

Without the `embed` feature, the model and instrument bank are loaded at runtime from the
repository's `models/` and `assets/` directories. This is convenient during development and
keeps rebuilds from producing a new binary hundreds of megabytes in size. Both release and
development builds use the same files installed by `scripts/download-assets.sh`.

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
files. Cleared results can no longer be played or exported.

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

The default test suite does not load the model weights or soundfont:

```bash
cargo test
```

Run model parity, end-to-end generation, and WAV export explicitly when the required assets and
enough memory are available:

```bash
cargo test --features heavy-tests -- --test-threads=1
```

The generation benchmark is ignored by default and can be started separately:

```bash
cargo test --release --features heavy-tests --test bench_gen -- --ignored --nocapture
```

Together, the light and heavy suites cover numerical parity of the model's forward pass,
byte-for-byte MIDI export, timeline and note-density correctness, WAV rendering, and end-to-end
generation.

## License

Except where otherwise noted, the source code in this repository is licensed under the
[Apache License 2.0](LICENSE).

The model checkpoint, SoundFont, and third-party Rust dependencies retain their respective
licenses and are not relicensed by this project. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
for details.
