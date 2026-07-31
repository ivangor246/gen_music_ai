#!/usr/bin/env bash

package_version() {
    local repository_root="$1"
    local version

    version="$(
        sed -n '/^\[package\]$/,/^\[/s/^version = "\([^"]*\)"$/\1/p' \
            "${repository_root}/Cargo.toml"
    )"
    if [[ -z "${version}" || "${version}" == *$'\n'* ]]; then
        echo "Unable to determine one package version from Cargo.toml." >&2
        return 1
    fi
    printf '%s\n' "${version}"
}
