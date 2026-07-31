//! Constrained token sampling, matching `MIDIModel.sample_top_p_k` and the
//! per-slot masking in the Python generation loop.
//!
//! Order (must match the reference): softmax(logits / temp) over the full vocab
//! -> multiply by the boolean legal-id mask (post-softmax) -> top-k -> top-p
//! nucleus -> renormalize -> multinomial. Operates on a plain `Vec<f32>` for
//! determinism and to avoid backend-specific multinomial differences.

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
}
