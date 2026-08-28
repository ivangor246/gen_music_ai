//! CPU and memory budgeting for generation.
//!
//! Generation is memory-bandwidth bound and will happily saturate every core
//! and every spare gigabyte, which on a small machine leaves nothing for the
//! desktop and can push the whole system into swap. Every decision about how
//! much of the machine to take lives here.

use std::sync::OnceLock;

use anyhow::{Result, bail};
use candle_core::DType;

use crate::core::model::config::{LlamaConfig, ModelConfig};
use crate::core::tokenizer::vocab::MAX_TOKEN_SEQ;

/// A run may claim at most this fraction of still-available memory for its
/// attention caches. The rest is headroom for the rest of the system.
const CACHE_MEMORY_SHARE: usize = 2;

/// Precision to load the checkpoint in.
///
/// f16 halves both the resident model (933 MiB -> 466 MiB) and the bytes read
/// per decode step. CPU generation is bound by that traffic rather than by
/// arithmetic, so it is the one lever that moves the decode step directly. f16
/// throughput depends on the CPU having usable native support -- hence a user
/// choice rather than a default.
///
/// `MIDI_MODEL_DTYPE` overrides `half_precision` for headless profiling.
pub fn weight_dtype(half_precision: bool) -> DType {
    match std::env::var("MIDI_MODEL_DTYPE").as_deref() {
        Ok("f16") => DType::F16,
        Ok("f32") => DType::F32,
        _ if half_precision => DType::F16,
        _ => DType::F32,
    }
}

static THREADS: OnceLock<usize> = OnceLock::new();

/// Threads to run tensor math on: half the logical cores, which on an SMT
/// machine is roughly the physical core count. This workload is bandwidth-bound
/// and stops scaling well before every core is busy, so the spare half buys
/// desktop responsiveness for very little throughput. `RAYON_NUM_THREADS`
/// overrides the choice.
pub fn compute_threads() -> usize {
    if let Some(requested) = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&value| value > 0)
    {
        return requested;
    }
    let total = std::thread::available_parallelism().map_or(1, |count| count.get());
    (total / 2).max(1)
}

/// Cap the global rayon pool that candle's matmul kernels run on, and report the
/// cap. Idempotent and safe to call from any thread: the first call wins, and a
/// pool somebody else already built is left alone.
pub fn configure_threads() -> usize {
    *THREADS.get_or_init(|| {
        let threads = compute_threads();
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
        threads
    })
}

/// Memory the platform reports as available, when it can. `MemAvailable`
/// already accounts for reclaimable page cache, so it is the honest number.
#[cfg(target_os = "linux")]
pub fn available_memory() -> Option<usize> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let kib: usize = line.split_whitespace().next()?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(not(target_os = "linux"))]
pub fn available_memory() -> Option<usize> {
    None
}

/// Bytes the attention caches hold for a run: keys and values, for every layer
/// of both stacks, over the full context window, for every track in the batch.
/// The caches inherit the model's dtype.
pub fn cache_bytes(config: &ModelConfig, batch: usize, context: usize, dtype: DType) -> usize {
    stack_cache_bytes(&config.net, batch, context, dtype)
        + stack_cache_bytes(&config.net_token, batch, MAX_TOKEN_SEQ, dtype)
}

fn stack_cache_bytes(config: &LlamaConfig, batch: usize, positions: usize, dtype: DType) -> usize {
    2 * config.num_hidden_layers
        * batch
        * config.num_attention_heads
        * positions
        * config.head_dim()
        * dtype.size_in_bytes()
}

/// Refuse a run whose caches would not fit rather than letting the machine swap
/// itself to a standstill. Platforms that cannot report free memory are trusted.
pub fn check_cache_budget(
    config: &ModelConfig,
    batch: usize,
    context: usize,
    dtype: DType,
) -> Result<()> {
    let needed = cache_bytes(config, batch, context, dtype);
    let Some(available) = available_memory() else {
        return Ok(());
    };
    let budget = available / CACHE_MEMORY_SHARE;
    if needed > budget {
        bail!(
            "this run needs {} of attention cache but only {} can be spared right now \
             ({} available): reduce the context window or the number of results",
            mib(needed),
            mib(budget),
            mib(available),
        );
    }
    Ok(())
}

fn mib(bytes: usize) -> String {
    format!("{} MiB", bytes / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ModelConfig {
        ModelConfig::from_compatible_json(crate::services::model_catalog::DEFAULT_CONFIG_JSON)
            .unwrap()
    }

    #[test]
    fn thread_cap_leaves_the_machine_usable() {
        let threads = compute_threads();
        assert!(threads >= 1);
        let total = std::thread::available_parallelism().map_or(1, |count| count.get());
        assert!(threads <= total);
    }

    #[test]
    fn cache_size_matches_the_hand_computed_figure() {
        // base: 2 (k+v) * 12 layers * 4 tracks * 16 heads * 512 pos * 64 dim * 4 B
        let base = 2 * 12 * 4 * 16 * 512 * 64 * 4;
        // token net: 3 layers, 4 heads, 8 positions, 256 dim
        let token = 2 * 3 * 4 * 4 * MAX_TOKEN_SEQ * 256 * 4;
        assert_eq!(cache_bytes(&config(), 4, 512, DType::F32), base + token);
    }

    #[test]
    fn cache_size_scales_with_batch_and_context() {
        let config = config();
        let single = cache_bytes(&config, 1, 512, DType::F32);
        assert_eq!(cache_bytes(&config, 4, 512, DType::F32), 4 * single);
        // Only the base stack grows with the context window.
        let wide = cache_bytes(&config, 1, 1024, DType::F32);
        assert!(wide > single && wide < 2 * single);
    }

    #[test]
    fn half_precision_halves_the_cache() {
        let config = config();
        assert_eq!(
            cache_bytes(&config, 4, 512, DType::F16) * 2,
            cache_bytes(&config, 4, 512, DType::F32)
        );
    }

    #[test]
    fn absurd_requests_are_refused() {
        // Rejection needs a memory reading; platforms without one trust the caller.
        if available_memory().is_none() {
            return;
        }
        assert!(check_cache_budget(&config(), 4, 512, DType::F32).is_ok());
        assert!(check_cache_budget(&config(), 4096, 4096, DType::F32).is_err());
    }
}
