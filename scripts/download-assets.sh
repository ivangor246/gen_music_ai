#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

readonly MODEL_URL="https://huggingface.co/skytnt/midi-model-tv2o-medium/resolve/0f8f265d4330f4e46527ac2313200254c5757f5f/model.safetensors"
readonly MODEL_SHA256="82ac8b2217f8f66f79737e444fe60c686d3cbfee54b0c8ef717f701213bbbb83"
readonly MODEL_PATH="${PROJECT_ROOT}/models/midi-model-tv2o-medium/model.safetensors"

readonly SOUNDFONT_URL="https://huggingface.co/skytnt/midi-model/resolve/1b01fa36e954cd5c3981119754675e8f88c99ab4/soundfont.sf2"
readonly SOUNDFONT_SHA256="5ea2375e8bd7d8e71def1036978c1621e85b66934169b6a2744b27b9b3c2d99c"
readonly SOUNDFONT_PATH="${PROJECT_ROOT}/assets/soundfont.sf2"

ASSET_TEMPORARY=""

cleanup_temporary() {
    if [[ -n "${ASSET_TEMPORARY}" ]]; then
        rm -f -- "${ASSET_TEMPORARY}"
    fi
}

trap cleanup_temporary EXIT

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        printf '%s\n' "Neither sha256sum nor shasum is available." >&2
        return 1
    fi
}

download_asset() {
    local name="$1"
    local url="$2"
    local expected_sha256="$3"
    local target="$4"
    local actual_sha256

    if [[ -f "${target}" ]]; then
        actual_sha256="$(sha256_file "${target}")"
        if [[ "${actual_sha256}" == "${expected_sha256}" ]]; then
            printf 'Verified %s: %s\n' "${name}" "${target}"
            return
        fi
        printf 'Replacing %s because its SHA-256 is invalid.\n' "${name}" >&2
    fi

    if ! command -v curl >/dev/null 2>&1; then
        printf '%s\n' "curl is required to download runtime assets." >&2
        return 1
    fi

    mkdir -p "$(dirname -- "${target}")"

    ASSET_TEMPORARY="$(mktemp "${target}.part.XXXXXX")"

    printf 'Downloading %s…\n' "${name}"
    curl --fail --location --retry 3 --progress-bar --output "${ASSET_TEMPORARY}" "${url}"

    actual_sha256="$(sha256_file "${ASSET_TEMPORARY}")"
    if [[ "${actual_sha256}" != "${expected_sha256}" ]]; then
        printf 'SHA-256 mismatch for %s.\nExpected: %s\nActual:   %s\n' \
            "${name}" "${expected_sha256}" "${actual_sha256}" >&2
        return 1
    fi

    mv "${ASSET_TEMPORARY}" "${target}"
    ASSET_TEMPORARY=""
    printf 'Installed %s: %s\n' "${name}" "${target}"
}

download_asset "MIDI model" "${MODEL_URL}" "${MODEL_SHA256}" "${MODEL_PATH}"
download_asset "SoundFont" "${SOUNDFONT_URL}" "${SOUNDFONT_SHA256}" "${SOUNDFONT_PATH}"
