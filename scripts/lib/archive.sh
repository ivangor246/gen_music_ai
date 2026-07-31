#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/checksum.sh"

ARCHIVE_TEMPORARY=""

cleanup_archive() {
    if [[ -n "${ARCHIVE_TEMPORARY}" && -d "${ARCHIVE_TEMPORARY}" ]]; then
        rm -rf -- "${ARCHIVE_TEMPORARY}"
    fi
}

trap cleanup_archive EXIT

create_archive_stage() {
    ARCHIVE_TEMPORARY="$(mktemp -d)"
}

write_archive_checksum() {
    local archive="$1"
    local checksum

    checksum="$(sha256_file "${archive}")"
    printf '%s  %s\n' "${checksum}" "$(basename -- "${archive}")" >"${archive}.sha256"
}
