//! Rotary positional embeddings (RoPE).
//!
//! inv_freq is not stored in the checkpoint, so we build cos/sin tables here.
//! The model was trained with HF `LlamaModel`, whose `rotate_half` uses
//! contiguous halves -> candle's `rotary_emb::rope` (not the interleaved
//! `rope_i`). Each stack has its own table because head_dim differs (64 vs 256).

use candle_core::{DType, Device, Result, Tensor};

pub struct RotaryCache {
    cos: Tensor,
    sin: Tensor,
}

impl RotaryCache {
    pub fn new(
        head_dim: usize,
        max_positions: usize,
        theta: f64,
        dtype: DType,
        device: &Device,
    ) -> Result<Self> {
        let half = head_dim / 2;
        let inv_freq: Vec<f32> = (0..half)
            .map(|i| (1.0 / theta.powf(2.0 * i as f64 / head_dim as f64)) as f32)
            .collect();
        let mut cos = Vec::with_capacity(max_positions * half);
        let mut sin = Vec::with_capacity(max_positions * half);
        for position in 0..max_positions {
            for &freq in &inv_freq {
                let angle = position as f32 * freq;
                cos.push(angle.cos());
                sin.push(angle.sin());
            }
        }
        // Kept in the model's dtype so `rope` never has to convert per step.
        Ok(Self {
            cos: Tensor::from_vec(cos, (max_positions, half), device)?.to_dtype(dtype)?,
            sin: Tensor::from_vec(sin, (max_positions, half), device)?.to_dtype(dtype)?,
        })
    }

    /// Apply RoPE to `x` (b, heads, seq, head_dim) for positions `offset..offset+seq`.
    pub fn apply(&self, x: &Tensor, offset: usize) -> Result<Tensor> {
        let seq = x.dim(2)?;
        let cos = self.cos.narrow(0, offset, seq)?;
        let sin = self.sin.narrow(0, offset, seq)?;
        candle_nn::rotary_emb::rope(&x.contiguous()?, &cos, &sin)
    }
}
