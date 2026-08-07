#!/usr/bin/env bash
# Run a command inside a memory-capped cgroup, with build and tensor-math
# parallelism held down to match.
#
# Building this tree with fat LTO, or running generation, can ask for more than
# the machine has free. Without a cap the kernel starts swapping and the desktop
# stops responding long before the OOM killer steps in; with one, the offending
# process is killed and nothing else notices.
#
#   scripts/capped.sh cargo test --release --features heavy-tests \
#       --test bench_gen -- --ignored --nocapture
#   scripts/capped.sh cargo run --release
#
# Overrides: CAP_MEMORY (e.g. 4G), CAP_JOBS, RAYON_NUM_THREADS.
set -euo pipefail

if [ $# -eq 0 ]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

cores=$(nproc 2>/dev/null || echo 2)
# Half the cores, the same split src/runtime.rs applies to tensor math.
threads=${RAYON_NUM_THREADS:-$(( cores / 2 > 0 ? cores / 2 : 1 ))}

available_kib=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo 2>/dev/null || echo 0)
# Leave a quarter of what is free to the rest of the system.
limit_mib=$(( available_kib * 3 / 4 / 1024 ))
[ "$limit_mib" -lt 2048 ] && limit_mib=2048

# rustc peaks near 2 GiB per job here, so memory decides how many run at once,
# not the core count -- one job per core is exactly what exhausts the machine.
mem_jobs=$(( limit_mib / 2048 ))
[ "$mem_jobs" -lt 1 ] && mem_jobs=1
jobs=${CAP_JOBS:-$(( mem_jobs < threads ? mem_jobs : threads ))}

CAP_MEMORY=${CAP_MEMORY:-${limit_mib}M}

export RAYON_NUM_THREADS="$threads"
export CARGO_BUILD_JOBS="$jobs"

echo "capped: memory=${CAP_MEMORY} jobs=${jobs} threads=${threads}" >&2

if ! command -v systemd-run >/dev/null 2>&1; then
    echo "warning: systemd-run missing, running without a memory cap" >&2
    exec "$@"
fi

# MemorySwapMax=0 keeps the cap meaningful: without it the cgroup just swaps.
exec systemd-run --user --scope -q --expand-environment=no \
    -p MemoryMax="$CAP_MEMORY" -p MemorySwapMax=0 \
    -- "$@"
