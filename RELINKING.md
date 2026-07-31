# Relinking with a Modified OxiSynth

The application links OxiSynth statically. OxiSynth 0.1.0, OxiSynth Chorus
0.1.0, and OxiSynth Reverb 0.1.0 are licensed under the GNU Lesser General
Public License version 2.1. The application source is provided so that a
recipient can modify these libraries and relink the executable.

The unmodified packages used by this project come from OxiSynth repository
commit `87c08c1a0165c7ea3577d86571f7688622387674`. Their published source archives
and the related `soundfont` package can be downloaded and verified with:

```bash
bash scripts/download-oxisynth-sources.sh
```

Binary distributions must provide these source archives alongside the binary
and must match the application source release used to build that binary.

## Prepare the Sources

Extract all four archives into one directory:

```bash
mkdir -p target/oxisynth-work
for archive in target/oxisynth-sources/*.crate; do
    tar -xzf "$archive" -C target/oxisynth-work
done
```

The expected directories are:

- `target/oxisynth-work/oxisynth-0.1.0`
- `target/oxisynth-work/oxisynth-chorus-0.1.0`
- `target/oxisynth-work/oxisynth-reverb-0.1.0`
- `target/oxisynth-work/soundfont-0.1.0`

Modify the OxiSynth sources as needed. Preserve the LGPL notices, document
changed files and dates, and license modifications to the LGPL components as
required by LGPL-2.1.

## Relink the Application

From the application source directory, override the crates.io packages with
the extracted local packages and build the executable:

```bash
cargo \
    --config 'patch.crates-io.oxisynth.path="target/oxisynth-work/oxisynth-0.1.0"' \
    --config 'patch.crates-io.oxisynth-chorus.path="target/oxisynth-work/oxisynth-chorus-0.1.0"' \
    --config 'patch.crates-io.oxisynth-reverb.path="target/oxisynth-work/oxisynth-reverb-0.1.0"' \
    --config 'patch.crates-io.soundfont.path="target/oxisynth-work/soundfont-0.1.0"' \
    build --release --features embed
```

Omit `--features embed` for a build that loads the model checkpoint and
SoundFont from disk. Runtime assets can be restored with
`scripts/download-assets.sh`.

The full LGPL-2.1 text is in [`licenses/LGPL-2.1.txt`](licenses/LGPL-2.1.txt).
The project's Apache-2.0 license does not restrict modification for personal
use or reverse engineering for debugging those modifications.
