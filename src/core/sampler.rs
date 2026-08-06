//! Constrained token sampling with per-slot masking.
//!
//! Order (must match the reference): softmax(logits / temp) over the full vocab
//! -> multiply by the boolean legal-id mask (post-softmax) -> top-k -> top-p
//! nucleus -> renormalize -> multinomial. Operates on a plain `Vec<f32>` for
//! determinism and to avoid backend-specific multinomial differences.
//!
//! Repetition penalty and n-gram banning below are a deliberate addition on
//! top of the reference algorithm: without them the model easily collapses
//! into looping the same short phrase, since a repeated tail is fed straight
//! back in as context and reinforces itself.

use std::collections::HashSet;

use rand::Rng;

/// Softmax with temperature over raw logits.
pub fn softmax_with_temp(logits: &[f32], temperature: f32) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<f32> = logits
        .iter()
        .map(|&l| ((l - max) / temperature).exp())
        .collect();
    let sum: f32 = probs.iter().sum();
    if sum > 0.0 {
        for p in &mut probs {
            *p /= sum;
        }
    }
    probs
}

/// Discourage ids seen recently: divide positive logits (multiply negative ones)
/// by `penalty` for every unique id in `recent`. `penalty <= 1.0` is a no-op.
/// Applied before softmax so it reshapes the whole distribution, not just the
/// sampled tail.
pub fn apply_repetition_penalty(logits: &mut [f32], recent: &[u32], penalty: f32) {
    if penalty <= 1.0 {
        return;
    }
    let unique: HashSet<u32> = recent.iter().copied().collect();
    for id in unique {
        if let Some(logit) = logits.get_mut(id as usize) {
            *logit = if *logit > 0.0 {
                *logit / penalty
            } else {
                *logit * penalty
            };
        }
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

/// Zero out every probability whose id is not in `allowed`.
pub fn apply_mask(probs: &mut [f32], allowed: &[u32]) {
    let mut keep = vec![false; probs.len()];
    for &id in allowed {
        keep[id as usize] = true;
    }
    for (p, &k) in probs.iter_mut().zip(keep.iter()) {
        if !k {
            *p = 0.0;
        }
    }
}

/// Greedy pick: highest masked probability (used for parity checks).
pub fn argmax(probs: &[f32]) -> u32 {
    let mut best = 0u32;
    let mut best_p = f32::NEG_INFINITY;
    for (id, &p) in probs.iter().enumerate() {
        if p > best_p {
            best_p = p;
            best = id as u32;
        }
    }
    best
}

/// top-k + top-p nucleus filter then multinomial draw, matching the reference.
pub fn sample_top_p_k<R: Rng>(probs: &[f32], top_p: f32, top_k: usize, rng: &mut R) -> u32 {
    let k = top_k.clamp(1, probs.len());
    // Sort ids by probability, descending, keep the top k.
    let mut order: Vec<u32> = (0..probs.len() as u32).collect();
    order.sort_unstable_by(|&a, &b| {
        probs[b as usize]
            .partial_cmp(&probs[a as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    order.truncate(k);

    // top-p: drop entries where the cumulative mass *before* them already exceeds p.
    let mut kept: Vec<(u32, f32)> = Vec::with_capacity(k);
    let mut cumulative = 0.0f32;
    for &id in &order {
        let p = probs[id as usize];
        if cumulative > top_p {
            break;
        }
        kept.push((id, p));
        cumulative += p;
    }

    let total: f32 = kept.iter().map(|&(_, p)| p).sum();
    if total <= 0.0 {
        return order.first().copied().unwrap_or(0);
    }
    let threshold = rng.gen_range(0.0f32..total);
    let mut acc = 0.0f32;
    for &(id, p) in &kept {
        acc += p;
        if acc >= threshold {
            return id;
        }
    }
    kept.last().map(|&(id, _)| id).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_restricts_argmax() {
        let logits = vec![0.1, 5.0, 0.2, 3.0];
        let mut probs = softmax_with_temp(&logits, 1.0);
        apply_mask(&mut probs, &[0, 2, 3]);
        // id 1 (largest) is masked out, so id 3 wins.
        assert_eq!(argmax(&probs), 3);
    }

    #[test]
    fn repetition_penalty_demotes_recent_ids() {
        let mut logits = vec![5.0, 4.9, 0.1];
        apply_repetition_penalty(&mut logits, &[0], 1.5);
        // id 0 was ahead of id 1; the penalty should flip the ranking.
        assert!(logits[1] > logits[0]);
    }

    #[test]
    fn repetition_penalty_noop_below_one() {
        let mut logits = vec![5.0, -2.0];
        let before = logits.clone();
        apply_repetition_penalty(&mut logits, &[0, 1], 1.0);
        assert_eq!(logits, before);
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
