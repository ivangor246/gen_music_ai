# Release Checklist

Use this checklist for every published version.

## Prepare

- Confirm the working tree contains only intended release changes.
- Update `version` in `Cargo.toml` and regenerate `Cargo.lock` if Cargo changes the package entry.
- Move completed entries from `Unreleased` in `CHANGELOG.md` to a versioned section with the
  release date.
- Confirm README screenshots and user instructions match the release interface.
- Confirm GitHub private vulnerability reporting is enabled in the repository security settings.
- Review `THIRD_PARTY_NOTICES.md`, `RELINKING.md`, `deny.toml`, and bundled license texts.

## Validate

Run the lightweight local checks:

```bash
cargo fmt --all -- --check
bash scripts/capped.sh cargo clippy --locked --all-targets -- -D warnings
bash scripts/test.sh
bash scripts/capped.sh cargo check --locked --features embed-soundfont
bash scripts/capped.sh cargo deny --all-features --locked check licenses
```

Trigger the Release workflow manually and inspect every generated binary archive, the source
archive, and their checksum files. Confirm at least one generated MIDI and WAV file on each
supported platform when suitable test machines are available. On a clean profile, confirm the
model list appears, **Download & Load** verifies and loads the checkpoint, a paused download can
resume, and confirmed removal deletes only the selected model.

## Publish

- Commit the version and changelog updates.
- Create and push an annotated `v<version>` tag that exactly matches `Cargo.toml`.
- Wait for all release jobs and checksum verification to succeed.
- Confirm the GitHub Release contains Linux, Windows, macOS, and corresponding source archives.
- Verify the published checksums from a fresh download.
- Review the generated release notes and add any important upgrade or known-issue information.

## After Release

- Confirm the README download link resolves to the new release.
- Start a new `Unreleased` section in `CHANGELOG.md` when the next change is made.
- Record release-specific limitations that should be addressed before the next version.
