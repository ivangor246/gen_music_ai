# Third-Party Notices

The Apache License 2.0 applied to this repository does not replace the licenses of the
third-party components listed below.

## SkyTNT MIDI Model

Parts of this project are based on the MIDI model implementation published by SkyTNT.

- Source: <https://github.com/SkyTNT/midi-model>
- License: Apache License 2.0

The `midi-model-tv2o-medium` checkpoint downloaded by `scripts/download-assets.sh` is also
published under the Apache License 2.0.

- Model: <https://huggingface.co/skytnt/midi-model-tv2o-medium>

## MuseScore General HQ SoundFont

The SoundFont downloaded by `scripts/download-assets.sh` identifies itself as
MuseScore General HQ v0.2 and is published under the MIT License. It is downloaded from a
pinned revision of the upstream SkyTNT asset repository and is not relicensed by this project.

- Asset: <https://huggingface.co/skytnt/midi-model/blob/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2>
- License information: <https://musescore.org/en/handbook/3/soundfonts-and-sfz-files>

## OxiSynth

Audio synthesis uses OxiSynth 0.1.0, which is licensed under the GNU Lesser General Public
License version 2.1 (LGPL-2.1).

- Source: <https://github.com/PolyMeilex/oxisynth>
- Package: <https://crates.io/crates/oxisynth/0.1.0>

Distributors of compiled binaries are responsible for satisfying the LGPL-2.1 requirements,
including the applicable license, source-code, modification, and relinking provisions.

Other Rust dependencies retain the licenses declared by their respective packages.
