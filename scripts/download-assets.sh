#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

source "${SCRIPT_DIR}/lib/download.sh"

readonly MODEL_URL="https://huggingface.co/skytnt/midi-model-tv2o-medium/resolve/0f8f265d4330f4e46527ac2313200254c5757f5f/model.safetensors"
readonly MODEL_SHA256="82ac8b2217f8f66f79737e444fe60c686d3cbfee54b0c8ef717f701213bbbb83"
readonly MODEL_PATH="${PROJECT_ROOT}/models/midi-model-tv2o-medium/model.safetensors"

readonly SOUNDFONT_URL="https://huggingface.co/skytnt/midi-model/resolve/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2"
readonly SOUNDFONT_SHA256="5ea2375e8bd7d8e71def1036978c1621e85b66934169b6a2744b27b9b3c2d99c"
readonly SOUNDFONT_PATH="${PROJECT_ROOT}/assets/soundfont.sf2"

download_verified_file "MIDI model" "${MODEL_URL}" "${MODEL_SHA256}" "${MODEL_PATH}"
download_verified_file "SoundFont" "${SOUNDFONT_URL}" "${SOUNDFONT_SHA256}" "${SOUNDFONT_PATH}"
