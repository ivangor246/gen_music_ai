//! Constrained token sampling.
//!
//! Order (must match the reference): softmax(logits / temp) with the whole vocab
//! in the denominator -> keep only legal ids -> top-k -> top-p nucleus ->
//! renormalize -> multinomial. Only the legal ids ever get exponentiated,
//! sorted or drawn, since a slot's legal set is usually a handful of ids out of
//! `VOCAB_SIZE`. Operates on plain slices for determinism and to avoid
//! backend-specific multinomial differences.
//!
//! Repetition penalty and n-gram banning below are a deliberate addition on
//! top of the reference algorithm: without them the model easily collapses
//! into looping the same short phrase, since a repeated tail is fed straight
//! back in as context and reinforces itself.

use std::cmp::Ordering;

use rand::Rng;

use crate::core::tokenizer::vocab::PAD_ID;

pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
}

/// Rolling window of the raw ids a track sampled most recently, kept alongside a
/// dense per-id counter so the repetition penalty tests membership with an index
/// instead of rebuilding a set on every slot.
pub struct TokenHistory {
    recent: Vec<u32>,
    counts: Vec<u16>,
    window: usize,
}

impl TokenHistory {
    pub fn new(vocab_size: usize, window: usize) -> Self {
        Self {
            recent: Vec::with_capacity(window),
            counts: vec![0; vocab_size],
            window,
        }
    }

    /// Record a sampled id, evicting the oldest once the window is full.
    pub fn push(&mut self, id: u32) {
        if let Some(count) = self.counts.get_mut(id as usize) {
            *count += 1;
        }
        self.recent.push(id);
        while self.recent.len() > self.window {
            let evicted = self.recent.remove(0);
            if let Some(count) = self.counts.get_mut(evicted as usize) {
                *count -= 1;
            }
        }
    }

    pub fn ids(&self) -> &[u32] {
        &self.recent
    }

    fn contains(&self, id: u32) -> bool {
        self.counts.get(id as usize).is_some_and(|&count| count > 0)
    }
}

/// Ids that would exactly complete a previously-seen `n`-gram given the tail of
/// `history`, i.e. the classic no-repeat-ngram guard: if the last `n - 1` ids
/// already occurred earlier in `history`, whatever followed them there is
/// banned now, since sampling it again would recreate that exact run. Blocks
/// literal copy-paste loops of a whole phrase without touching short,
/// legitimate repeats (a single held note, a drum backbeat) shorter than `n`.
pub fn no_repeat_ngram_bans(history: &[u32], n: usize) -> Vec<u32> {
    let mut banned = Vec::new();
    if n < 2 || history.len() < n {
        return banned;
    }
    let recent = &history[history.len() - (n - 1)..];
    for start in 0..history.len() - (n - 1) {
        let end = start + (n - 1);
        if end >= history.len() {
            break;
        }
        if &history[start..end] == recent {
            banned.push(history[end]);
        }
    }
    banned
}

/// Draw one id from `allowed`. `history` and `penalty` drive the repetition
/// penalty, which reshapes the logits before the softmax so it affects the
/// whole distribution rather than just the sampled tail.
pub fn sample_constrained<R: Rng>(
    logits: &[f32],
    allowed: &[u32],
    history: &TokenHistory,
    penalty: f32,
    params: &SamplingParams,
    rng: &mut R,
) -> u32 {
    // Discourage ids seen recently: divide positive logits (multiply negative
    // ones) by `penalty`. `penalty <= 1.0` is a no-op.
    let logit = |id: u32| -> f32 {
        let raw = logits
            .get(id as usize)
            .copied()
            .unwrap_or(f32::NEG_INFINITY);
        if penalty > 1.0 && history.contains(id) {
            if raw > 0.0 {
                raw / penalty
            } else {
                raw * penalty
            }
        } else {
            raw
        }
    };

    // The softmax denominator spans the whole vocab, illegal ids included, as
    // in the reference; only the numerators are restricted to `allowed`.
    let vocab = logits.len() as u32;
    let mut max = f32::NEG_INFINITY;
    for id in 0..vocab {
        max = max.max(logit(id));
    }
    let mut sum = 0.0f32;
    for id in 0..vocab {
        sum += ((logit(id) - max) / params.temperature).exp();
    }

    let mut candidates: Vec<(u32, f32)> = allowed
        .iter()
        .map(|&id| {
            let weight = ((logit(id) - max) / params.temperature).exp();
            (id, if sum > 0.0 { weight / sum } else { 0.0 })
        })
        .collect();
    candidates.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    candidates.truncate(params.top_k.clamp(1, candidates.len().max(1)));

    // top-p: drop entries where the cumulative mass *before* them already exceeds p.
    let mut total = 0.0f32;
    let mut kept = 0usize;
    for &(_, probability) in &candidates {
        if total > params.top_p {
            break;
        }
        total += probability;
        kept += 1;
    }

    if total <= 0.0 {
        // Every legal id underflowed to zero -- a low temperature is enough,
        // since f32 `exp` flushes below about -103. Falling back to the global
        // argmax here would hand back exactly the id the mask exists to forbid.
        return allowed
            .iter()
            .copied()
            .max_by(|&a, &b| logit(a).total_cmp(&logit(b)))
            .unwrap_or(PAD_ID);
    }

    let threshold = rng.gen_range(0.0f32..total);
    let mut acc = 0.0f32;
    for &(id, probability) in &candidates[..kept] {
        acc += probability;
        if acc >= threshold {
            return id;
        }
    }
    candidates[kept - 1].0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn params(temperature: f32) -> SamplingParams {
        SamplingParams {
            temperature,
            top_p: 1.0,
            top_k: 1,
        }
    }

    fn history(vocab: usize, ids: &[u32]) -> TokenHistory {
        let mut history = TokenHistory::new(vocab, 8);
        for &id in ids {
            history.push(id);
        }
        history
    }

    #[test]
    fn mask_restricts_the_draw() {
        let logits = vec![0.1, 5.0, 0.2, 3.0];
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        // id 1 (largest) is masked out, so id 3 wins.
        let id = sample_constrained(
            &logits,
            &[0, 2, 3],
            &history(4, &[]),
            1.0,
            &params(1.0),
            &mut rng,
        );
        assert_eq!(id, 3);
    }

    #[test]
    fn repetition_penalty_demotes_recent_ids() {
        let logits = vec![5.0, 4.9, 0.1];
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        // id 0 was ahead of id 1; the penalty should flip the ranking.
        let id = sample_constrained(
            &logits,
            &[0, 1, 2],
            &history(3, &[0]),
            1.5,
            &params(1.0),
            &mut rng,
        );
        assert_eq!(id, 1);
    }

    #[test]
    fn repetition_penalty_noop_below_one() {
        let logits = vec![5.0, 4.9, 0.1];
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let id = sample_constrained(
            &logits,
            &[0, 1, 2],
            &history(3, &[0]),
            1.0,
            &params(1.0),
            &mut rng,
        );
        assert_eq!(id, 0);
    }

    #[test]
    fn underflowing_probabilities_stay_inside_the_mask() {
        // At temperature 0.1 both legal ids underflow to exactly 0.0 while the
        // masked-out id 0 keeps all the mass. The draw must still be legal.
        let logits = vec![100.0, 0.0, -1.0];
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let id = sample_constrained(
            &logits,
            &[1, 2],
            &history(3, &[]),
            1.0,
            &params(0.1),
            &mut rng,
        );
        assert_eq!(id, 1);
    }

    #[test]
    fn history_window_forgets_evicted_ids() {
        let mut history = TokenHistory::new(16, 3);
        for id in [1u32, 2, 3, 4] {
            history.push(id);
        }
        assert_eq!(history.ids(), [2, 3, 4]);
        assert!(!history.contains(1));
        assert!(history.contains(4));
    }

    #[test]
    fn history_counts_repeated_ids() {
        let mut history = TokenHistory::new(16, 3);
        for id in [5u32, 5, 6, 7] {
            history.push(id);
        }
        // One of the two 5s was evicted, so 5 is still penalized.
        assert!(history.contains(5));
        history.push(8);
        assert!(!history.contains(5));
    }

    #[test]
    fn ngram_ban_flags_repeated_completion() {
        // ids 1,2,3 occurred once already; now 1,2 recur, so 3 should be banned.
        let history = vec![9, 1, 2, 3, 8, 1, 2];
        let banned = no_repeat_ngram_bans(&history, 3);
        assert_eq!(banned, vec![3]);
    }

    #[test]
    fn ngram_ban_empty_without_history() {
        let history = vec![1, 2];
        assert!(no_repeat_ngram_bans(&history, 3).is_empty());
    }
}
