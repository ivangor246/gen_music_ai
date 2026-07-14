//! Key/value caches for autoregressive decoding.
//!
//! The base net keeps a `StackCache` persistent across a generation section;
//! the token net uses a fresh one per event. `reset` lets callers reuse the
//! allocation instead of rebuilding the vector each time.

use candle_core::{Result, Tensor};

#[derive(Default)]
pub struct LayerKvCache {
    kv: Option<(Tensor, Tensor)>,
}

impl LayerKvCache {
    /// Append new keys/values (b, heads, seq, head_dim) and return the full pair.
    pub fn append(&mut self, k: &Tensor, v: &Tensor) -> Result<(Tensor, Tensor)> {
        let pair = match self.kv.take() {
            Some((prev_k, prev_v)) => (
                Tensor::cat(&[&prev_k, k], 2)?,
                Tensor::cat(&[&prev_v, v], 2)?,
            ),
            None => (k.clone(), v.clone()),
        };
        self.kv = Some((pair.0.clone(), pair.1.clone()));
        Ok(pair)
    }

    fn reset(&mut self) {
        self.kv = None;
    }
}

pub struct StackCache {
    layers: Vec<LayerKvCache>,
}

impl StackCache {
    pub fn new(num_layers: usize) -> Self {
        Self {
            layers: (0..num_layers).map(|_| LayerKvCache::default()).collect(),
        }
    }

    pub fn layer(&mut self, index: usize) -> &mut LayerKvCache {
        &mut self.layers[index]
    }

    /// Number of cached positions (identical across layers).
    pub fn len(&self) -> usize {
        self.layers
            .first()
            .and_then(|layer| layer.kv.as_ref())
            .map(|(k, _)| k.dim(2).unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.reset();
        }
    }
}
