# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Built-in model catalog with selection, resumable download progress, checksum verification,
  explicit loading, cancellation, retry, and confirmed local removal.
- Lightweight Linux, Windows, and macOS CI coverage for the external model workflow.

### Changed

- Model checkpoints are stored independently in the platform application data directory and are
  no longer compiled into release binaries; release builds embed only the SoundFont.
- Model configuration compatibility is validated before Candle constructs the network.
- Removed model-loading and asset-heavy test targets that could exhaust development machines;
  remaining tests use serialized builds, omit debug symbols, and have a fail-closed Linux cgroup
  runner capped at 3 GiB.

## [0.1.0] - 2026-07-31

### Added

- Local CPU music generation with configurable General MIDI instruments, track structure, and
  sampling parameters.
- Built-in and user-defined presets, instrument search, family grouping, and selected-instrument
  tags.
- Batched candidate generation with reproducible seeds, progress reporting, and cancellation.
- Result cards with playback, seeking, note-density visualization, and direct MIDI or WAV export.
- Disk-backed generation cache and platform-specific storage for presets and application settings.
- Adaptive desktop interface and application packaging for x86-64 Linux, Windows, and macOS.
- Reproducible runtime asset downloads with pinned revisions and SHA-256 verification.
- Lightweight and opt-in asset-heavy test suites, dependency license auditing, and release CI.
- Self-contained binary archives, corresponding source archives, and checksum verification.

### Security

- Added request limits for composition duration and event budgets.
- Added explicit confirmation before generated cache data is removed.
- Added validation for release tags, package versions, archive targets, and downloaded assets.

[Unreleased]: https://github.com/ivangor246/gen_music_ai/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ivangor246/gen_music_ai/releases/tag/v0.1.0
