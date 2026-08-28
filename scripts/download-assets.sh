#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

source "${SCRIPT_DIR}/lib/download.sh"

readonly SOUNDFONT_URL="https://huggingface.co/skytnt/midi-model/resolve/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2"
readonly SOUNDFONT_SHA256="5ea2375e8bd7d8e71def1036978c1621e85b66934169b6a2744b27b9b3c2d99c"
readonly SOUNDFONT_PATH="${PROJECT_ROOT}/assets/soundfont.sf2"

if [[ $# -ne 0 ]]; then
    echo "Usage: $0" >&2
    exit 2
fi

download_verified_file "SoundFont" "${SOUNDFONT_URL}" "${SOUNDFONT_SHA256}" "${SOUNDFONT_PATH}"
