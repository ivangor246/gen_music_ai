#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly DESTINATION="${1:-${PROJECT_ROOT}/target/option-ext-source}"
readonly PACKAGE="option-ext"
readonly VERSION="0.2.0"
readonly FILENAME="${PACKAGE}-${VERSION}.crate"
readonly URL="https://static.crates.io/crates/${PACKAGE}/${FILENAME}"
readonly SHA256="04744f49eae99ab78e0d5c0b603ab218f515ea8cfe5a456d7629ad883a3b6e7d"

source "${SCRIPT_DIR}/lib/download.sh"

download_verified_file +    "${PACKAGE} ${VERSION} source" +    "${URL}" +    "${SHA256}" +    "${DESTINATION}/${FILENAME}"
