#!/usr/bin/env bash
# Run a command inside a conservative, non-overridable memory cgroup.
# The wrapper fails closed when Linux cannot enforce the limit.
set -euo pipefail

if [ $# -eq 0 ]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

if ! command -v systemd-run >/dev/null 2>&1; then
    echo "error: systemd-run is required; refusing to run without a memory cap" >&2
    exit 1
fi
if ! command -v flock >/dev/null 2>&1; then
    echo "error: flock is required; refusing an unsafe concurrent run" >&2
    exit 1
fi
if [[ -z ${XDG_RUNTIME_DIR:-} || ! -d $XDG_RUNTIME_DIR ]]; then
    echo "error: XDG_RUNTIME_DIR is required for the safety lock" >&2
    exit 1
fi

exec 9>"${XDG_RUNTIME_DIR}/gen_music_ai-capped.lock"
if ! flock -n 9; then
    echo "error: another capped project command is already running" >&2
    exit 1
fi

total_kib=$(awk '/^MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
available_kib=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
limit_mib=$(( total_kib / 4 / 1024 ))
available_limit_mib=$(( available_kib / 2 / 1024 ))
if [ "$available_limit_mib" -lt "$limit_mib" ]; then
    limit_mib=$available_limit_mib
fi
if [ "$limit_mib" -lt 512 ]; then
    echo "error: insufficient memory for a safely capped command" >&2
    exit 1
fi
if [ "$limit_mib" -gt 3072 ]; then
    limit_mib=3072
fi

export CARGO_BUILD_JOBS=1
export RAYON_NUM_THREADS=1

echo "capped: memory=${limit_mib}M jobs=1 threads=1 swap=off" >&2

exec systemd-run --user --scope -q --expand-environment=no \
    -p MemoryMax="${limit_mib}M" -p MemorySwapMax=0 \
    -- "$@"
