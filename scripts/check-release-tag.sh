#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <release-tag>" >&2
    exit 1
fi

readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RELEASE_TAG="$1"
source "${REPOSITORY_ROOT}/scripts/lib/package-metadata.sh"

PACKAGE_VERSION="$(package_version "${REPOSITORY_ROOT}")"
readonly PACKAGE_VERSION
readonly EXPECTED_TAG="v${PACKAGE_VERSION}"

if [[ "${RELEASE_TAG}" != "${EXPECTED_TAG}" ]]; then
    echo "Release tag ${RELEASE_TAG} does not match package version ${EXPECTED_TAG}." >&2
    exit 1
fi

echo "Release tag ${RELEASE_TAG} matches Cargo.toml."
