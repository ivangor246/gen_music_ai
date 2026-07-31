#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "Usage: $0 <binary> <version> <target> <output-directory>" >&2
    exit 1
fi

readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "${REPOSITORY_ROOT}/scripts/lib/archive.sh"

readonly BINARY_PATH="$1"
readonly VERSION="$2"
readonly TARGET="$3"
readonly OUTPUT_DIRECTORY="$4"

if [[ ! -f "${BINARY_PATH}" ]]; then
    echo "Release binary not found: ${BINARY_PATH}" >&2
    exit 1
fi

if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]]; then
    echo "Invalid release version: ${VERSION}" >&2
    exit 1
fi

if [[ ! "${TARGET}" =~ ^[0-9A-Za-z_.-]+$ ]]; then
    echo "Invalid target name: ${TARGET}" >&2
    exit 1
fi

readonly PACKAGE_NAME="gen_music_ai-${VERSION}-${TARGET}"

create_archive_stage
readonly PACKAGE_DIRECTORY="${ARCHIVE_TEMPORARY}/${PACKAGE_NAME}"
readonly SOURCE_DIRECTORY="${PACKAGE_DIRECTORY}/third-party-sources"

mkdir -p -- "${PACKAGE_DIRECTORY}" "${SOURCE_DIRECTORY}" "${OUTPUT_DIRECTORY}"
cp -- "${BINARY_PATH}" "${PACKAGE_DIRECTORY}/"
cp -- \
    "${REPOSITORY_ROOT}/LICENSE" \
    "${REPOSITORY_ROOT}/README.md" \
    "${REPOSITORY_ROOT}/RELINKING.md" \
    "${REPOSITORY_ROOT}/THIRD_PARTY_NOTICES.md" \
    "${PACKAGE_DIRECTORY}/"
cp -R -- "${REPOSITORY_ROOT}/licenses" "${PACKAGE_DIRECTORY}/licenses"

bash "${REPOSITORY_ROOT}/scripts/download-oxisynth-sources.sh" "${SOURCE_DIRECTORY}"
bash "${REPOSITORY_ROOT}/scripts/download-option-ext-source.sh" "${SOURCE_DIRECTORY}"

readonly ARCHIVE="${OUTPUT_DIRECTORY}/${PACKAGE_NAME}.tar.gz"
tar -czf "${ARCHIVE}" -C "${ARCHIVE_TEMPORARY}" "${PACKAGE_NAME}"
write_archive_checksum "${ARCHIVE}"

echo "Created ${ARCHIVE}"
