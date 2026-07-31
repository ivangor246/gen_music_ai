#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <release-tag>" >&2
    exit 1
fi

readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RELEASE_TAG="$1"

PACKAGE_VERSION="$(
    sed -n '/^\[package\]$/,/^\[/s/^version = "\([^"]*\)"$/\1/p' \
        "${REPOSITORY_ROOT}/Cargo.toml"
)"

if [[ -z "${PACKAGE_VERSION}" || "${PACKAGE_VERSION}" == *$'\n'* ]]; then
    echo "Unable to determine one package version from Cargo.toml." >&2
    exit 1
fi

readonly EXPECTED_TAG="v${PACKAGE_VERSION}"

if [[ "${RELEASE_TAG}" != "${EXPECTED_TAG}" ]]; then
    echo "Release tag ${RELEASE_TAG} does not match package version ${EXPECTED_TAG}." >&2
    exit 1
fi

echo "Release tag ${RELEASE_TAG} matches Cargo.toml."
