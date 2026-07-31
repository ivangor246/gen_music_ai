#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/checksum.sh"

DOWNLOAD_TEMPORARY=""

cleanup_download() {
    if [[ -n "${DOWNLOAD_TEMPORARY}" ]]; then
        rm -f -- "${DOWNLOAD_TEMPORARY}"
    fi
}

trap cleanup_download EXIT

download_verified_file() {
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
        printf '%s\n' "curl is required to download files." >&2
        return 1
    fi

    mkdir -p "$(dirname -- "${target}")"
    DOWNLOAD_TEMPORARY="$(mktemp "${target}.part.XXXXXX")"

    printf 'Downloading %s…\n' "${name}"
    curl --fail --location --retry 3 --progress-bar --output "${DOWNLOAD_TEMPORARY}" "${url}"

    actual_sha256="$(sha256_file "${DOWNLOAD_TEMPORARY}")"
    if [[ "${actual_sha256}" != "${expected_sha256}" ]]; then
        printf 'SHA-256 mismatch for %s.\nExpected: %s\nActual:   %s\n' \
            "${name}" "${expected_sha256}" "${actual_sha256}" >&2
        return 1
    fi

    mv "${DOWNLOAD_TEMPORARY}" "${target}"
    DOWNLOAD_TEMPORARY=""
    printf 'Installed %s: %s\n' "${name}" "${target}"
}
