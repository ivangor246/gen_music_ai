# Third-Party Notices

The Apache License 2.0 applied to this repository does not replace the licenses of the
third-party components listed below.

## SkyTNT MIDI Model

Parts of this project are based on the MIDI model implementation published by SkyTNT.

- Source: <https://github.com/SkyTNT/midi-model>
- License: Apache License 2.0

The application catalog can download the `midi-model-tv2o-medium` checkpoint independently of
the executable. The checkpoint is also published under the Apache License 2.0.

- Model: <https://huggingface.co/skytnt/midi-model-tv2o-medium>

## MuseScore General HQ SoundFont

The SoundFont downloaded by `scripts/download-assets.sh` identifies itself as
MuseScore General HQ v0.2 and is published under the MIT License. It is downloaded from a
pinned revision of the upstream SkyTNT asset repository and is not relicensed by this project.

- Asset: <https://huggingface.co/skytnt/midi-model/blob/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2>
- License information: <https://musescore.org/en/handbook/3/soundfonts-and-sfz-files>
- License and attribution text:
  [`licenses/MUSESCORE_GENERAL_HQ-MIT.txt`](licenses/MUSESCORE_GENERAL_HQ-MIT.txt)

## MIDI-GPT

This project includes an independent Candle implementation of the GPT-2 inference architecture
and Yellow tokenizer grammar used by MIDI-GPT. The upstream MIDI-GPT source is MIT-licensed.

- Source: <https://github.com/metacreation/MIDI-GPT>
- Source license: MIT

The application catalog downloads the pinned `yellow_small-final.safetensors` checkpoint directly
from the upstream Hugging Face repository. The weights are not part of this repository or binary
distribution and remain licensed under Creative Commons Attribution-NonCommercial 4.0
International (CC BY-NC 4.0). They may not be used for commercial purposes.

- Model: <https://huggingface.co/Metacreation/MIDI-GPT>
- Model license: <https://creativecommons.org/licenses/by-nc/4.0/>

## OxiSynth

Audio synthesis uses OxiSynth 0.1.0, OxiSynth Chorus 0.1.0, and OxiSynth Reverb 0.1.0.
These packages are licensed under the GNU Lesser General Public License version 2.1
(LGPL-2.1) and are statically linked into compiled executables.

- Upstream revision:
  <https://github.com/PolyMeilex/oxisynth/tree/87c08c1a0165c7ea3577d86571f7688622387674>
- Package: <https://crates.io/crates/oxisynth/0.1.0>
- License: [`licenses/LGPL-2.1.txt`](licenses/LGPL-2.1.txt)
- Source and relinking instructions: [`RELINKING.md`](RELINKING.md)

The related `soundfont` 0.1.0 parser is licensed under the MIT License. Its license is in
[`licenses/SOUNDFONT-MIT.txt`](licenses/SOUNDFONT-MIT.txt).

Binary distributions must include this notice, the full LGPL-2.1 text, the matching application
source, and the verified OxiSynth source packages described in `RELINKING.md`. Distribution terms
must permit modification for personal use and reverse engineering for debugging those
modifications.

## webpki-root-certs

HTTPS certificate verification uses the Mozilla root certificate data distributed by
`webpki-root-certs` 1.0.9 under the Community Data License Agreement Permissive 2.0.

- Package: <https://crates.io/crates/webpki-root-certs/1.0.9>
- License: [`licenses/CDLA-Permissive-2.0.txt`](licenses/CDLA-Permissive-2.0.txt)

## option-ext

The `directories` dependency uses option-ext 0.2.0, which is licensed under the Mozilla Public
License version 2.0 (MPL-2.0).

- Upstream revision:
  <https://github.com/soc/option-ext/tree/272f22fc9ea1ac6b08f01704af52c4ac338df4e2>
- Package: <https://crates.io/crates/option-ext/0.2.0>
- License: [`licenses/MPL-2.0.txt`](licenses/MPL-2.0.txt)

The unmodified source archive can be downloaded and checksum-verified with
`scripts/download-option-ext-source.sh`. Binary distributions must make this source available
and state where recipients can obtain it.

Other Rust dependencies retain the licenses declared by their respective packages. The accepted
license policy for the complete locked dependency graph is defined in [`deny.toml`](deny.toml) and
enforced by the repository's license-audit workflow.
