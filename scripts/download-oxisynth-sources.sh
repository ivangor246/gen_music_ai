#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly DESTINATION="${1:-${PROJECT_ROOT}/target/oxisynth-sources}"
readonly CRATES_URL="https://static.crates.io/crates"

source "${SCRIPT_DIR}/lib/download.sh"

download_crate() {
    local package="$1"
    local version="$2"
    local sha256="$3"
    local filename="${package}-${version}.crate"

    download_verified_file \
        "${package} ${version} source" \
        "${CRATES_URL}/${package}/${filename}" \
        "${sha256}" \
        "${DESTINATION}/${filename}"
}

download_crate \
    "oxisynth" \
    "0.1.0" \
    "68d089fc07e074f57ac6698292ce3da05eac1d282b821eb4156d4c7d8e36c6ec"
download_crate \
    "oxisynth-chorus" \
    "0.1.0" \
    "755d6ea336fc37043b00a1b54198e24cb4537c509435c7aca2d837d36185703b"
download_crate \
    "oxisynth-reverb" \
    "0.1.0" \
    "bdbb2d72d399b03dc7546c1087fabcdba68402f987bea903fe284c946fcca7aa"
download_crate \
    "soundfont" \
    "0.1.0" \
    "2a7f4cb358863e55f8f1a3882f68601360cf6c42fc53ff2fe9aea41c33e24489"

printf 'OxiSynth source packages are ready in %s\n' "${DESTINATION}"
