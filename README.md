# gen_music_ai

`gen_music_ai` is a desktop application for generating multi-instrument music locally. Configure
instruments and generation parameters, preview the results, and export selected tracks as
Standard MIDI or WAV.

Generation runs entirely on the CPU and does not require a GPU or a remote inference service.

## Features

- Generate tracks with up to 15 selected General MIDI instruments, or let the model choose.
- Configure drum kit, tempo, time signature, key signature, length, and event budget.
- Tune temperature, top-p, top-k, result count, seed, control changes, and musical memory.
- Start from built-in style presets or save and delete custom presets.
- Generate several candidates with reproducible seeds and cancel an active generation.
- Preview a selected result with playback, seeking, and a note-density timeline.
- Export only the selected result as `.mid` or as 44.1 kHz, 16-bit stereo `.wav`.
- Keep long compositions in a disk-backed token cache instead of retaining their complete token
  history in memory.

## Requirements

- Rust 1.85 or newer (the project uses Rust edition 2024).
- `curl` and either `sha256sum` or `shasum` for downloading runtime assets.
- Approximately 520 MB of disk space for the model checkpoint and SoundFont, in addition to
  build artifacts.
- An audio output device for live playback.

On Debian and Ubuntu, install the ALSA development files required by `cpal`:

```bash
sudo apt install pkg-config libasound2-dev
```

Model loading and generation require substantially more memory than the checkpoint size because
the model weights are converted to `f32` for CPU inference. Larger result counts and musical
memory settings increase peak memory use. Generation speed depends on the CPU and selected
settings.

## Run from Source

Download the pinned model checkpoint and SoundFont after cloning the repository:

```bash
bash scripts/download-assets.sh
```

The script verifies SHA-256 checksums and skips files that are already present and valid. It
downloads:

- [`midi-model-tv2o-medium`](https://huggingface.co/skytnt/midi-model-tv2o-medium/tree/0f8f265d4330f4e46527ac2313200254c5757f5f)
  to `models/midi-model-tv2o-medium/model.safetensors`;
- the pinned [SoundFont](https://huggingface.co/skytnt/midi-model/blob/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2)
  to `assets/soundfont.sf2`.

Run the application using the committed dependency lockfile:

```bash
cargo run --locked
```

In the application:

1. Select **Load Model** and wait until the model is ready.
2. Choose a preset or configure the instruments and generation parameters.
3. Select **Generate**.
4. Choose a result, preview it, and save it as MIDI or WAV.

## Release Build

The default build reads the checkpoint and SoundFont from their repository paths at runtime.
This keeps development builds smaller and avoids embedding the assets after every code change:

```bash
cargo build --release --locked
```

Use the `embed` feature to include both runtime assets in the executable:

```bash
cargo build --release --locked --features embed
```

The embedded build does not require separate model or SoundFont files at runtime, but it is
larger and still depends on the operating system's supported audio and graphics facilities.
The assets are required before running the default build or compiling with the `embed` feature.

## User Data and Exports

Presets, the last save directory, and generated token caches are stored in the
platform-specific application data directory selected by the
[`directories`](https://crates.io/crates/directories) crate. They are not written next to the
executable.

MIDI and WAV files are created only after an explicit save action. The save dialog starts in the
system Downloads directory and remembers the last selected directory. Clearing the application
cache removes generated token histories and makes those unsaved results unavailable; it does not
delete previously exported files.

## Tests

The default suite covers unit tests and lightweight MIDI/timeline integration tests without
loading the model checkpoint or SoundFont:

```bash
cargo test --locked
```

Model parity, end-to-end generation, and WAV rendering are opt-in because they require the
downloaded assets and significantly more time and memory:

```bash
cargo test --locked --features heavy-tests -- --test-threads=1
```

The generation benchmark is ignored by default and can be run separately:

```bash
cargo test --release --locked --features heavy-tests --test bench_gen -- --ignored --nocapture
```

Check formatting with:

```bash
cargo fmt --all -- --check
```

## Project Structure

- `src/core/` — model implementation, sampling constraints, tokenizer, and MIDI encoding.
- `src/services/` — generation, token storage, playback, synthesis, export, presets, and settings.
- `src/ui/` — Iced application state, messages, tasks, views, and timeline visualization.
- `models/` — tracked model configuration and the downloaded checkpoint location.
- `assets/` — downloaded SoundFont location.
- `scripts/` — reproducible runtime asset download and checksum verification.
- `tests/` — lightweight tests, opt-in model tests, fixtures, and the generation benchmark.

## License

Except where otherwise noted, the source code in this repository is licensed under the
[Apache License 2.0](LICENSE).

This project is based in part on the Apache-2.0-licensed
[SkyTNT MIDI model](https://github.com/SkyTNT/midi-model). The model checkpoint, SoundFont, and
Rust dependencies retain their own licenses and are not relicensed by this project. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) before redistributing source code, assets, or
compiled binaries.
