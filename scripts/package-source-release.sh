#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <version> <output-directory>" >&2
    exit 1
fi

readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "${REPOSITORY_ROOT}/scripts/lib/archive.sh"

readonly VERSION="$1"
readonly OUTPUT_DIRECTORY="$2"

if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]]; then
    echo "Invalid release version: ${VERSION}" >&2
    exit 1
fi

readonly PACKAGE_NAME="gen_music_ai-${VERSION}-source"

create_archive_stage
readonly PACKAGE_DIRECTORY="${ARCHIVE_TEMPORARY}/${PACKAGE_NAME}"
readonly SOURCE_DIRECTORY="${PACKAGE_DIRECTORY}/third-party-sources"

mkdir -p -- "${PACKAGE_DIRECTORY}" "${SOURCE_DIRECTORY}" "${OUTPUT_DIRECTORY}"
git -C "${REPOSITORY_ROOT}" archive --format=tar HEAD \
    | tar -xf - -C "${PACKAGE_DIRECTORY}"

bash "${REPOSITORY_ROOT}/scripts/download-oxisynth-sources.sh" "${SOURCE_DIRECTORY}"
bash "${REPOSITORY_ROOT}/scripts/download-option-ext-source.sh" "${SOURCE_DIRECTORY}"

readonly ARCHIVE="${OUTPUT_DIRECTORY}/${PACKAGE_NAME}.tar.gz"
tar -czf "${ARCHIVE}" -C "${ARCHIVE_TEMPORARY}" "${PACKAGE_NAME}"
write_archive_checksum "${ARCHIVE}"

echo "Created ${ARCHIVE}"
