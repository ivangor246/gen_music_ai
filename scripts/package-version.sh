#!/usr/bin/env bash

set -euo pipefail

readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "${REPOSITORY_ROOT}/scripts/lib/package-metadata.sh"

package_version "${REPOSITORY_ROOT}"
