//! Key/value caches for autoregressive decoding.
//!
//! Each layer owns one preallocated `(b, heads, capacity, head_dim)` buffer per
//! side and writes new positions into it with `slice_set`, so a decode step
//! copies a single position instead of re-concatenating the whole cache.
//! `reset` rewinds the write cursor without dropping the buffers, which lets
//! the base net reuse one cache across sections and the token net across events.

use candle_core::{Result, Tensor};

#[derive(Default)]
pub struct LayerKvCache {
    buffers: Option<(Tensor, Tensor)>,
    capacity: usize,
    len: usize,
}

impl LayerKvCache {
    fn new(capacity: usize) -> Self {
        Self {
            buffers: None,
            capacity,
            len: 0,
        }
    }

    /// Append new keys/values (b, heads, seq, head_dim) and return the full pair
    /// as views over the buffers.
    pub fn append(&mut self, k: &Tensor, v: &Tensor) -> Result<(Tensor, Tensor)> {
        let seq = k.dim(2)?;
        if !self.fits(k, seq) {
            self.allocate(k, v, seq)?;
        }
        let Some((keys, values)) = &self.buffers else {
            candle_core::bail!("kv cache buffers were not allocated")
        };
        keys.slice_set(k, 2, self.len)?;
        values.slice_set(v, 2, self.len)?;
        self.len += seq;
        Ok((keys.narrow(2, 0, self.len)?, values.narrow(2, 0, self.len)?))
    }

    /// Whether the current buffers can take `seq` more positions of `k`'s shape.
    fn fits(&self, k: &Tensor, seq: usize) -> bool {
        self.len + seq <= self.capacity && self.matches(k)
    }

    /// Same batch, heads, head_dim and dtype as the allocated buffers.
    fn matches(&self, k: &Tensor) -> bool {
        let Some((keys, _)) = &self.buffers else {
            return false;
        };
        keys.dtype() == k.dtype()
            && keys.dims().len() == k.dims().len()
            && keys
                .dims()
                .iter()
                .zip(k.dims())
                .enumerate()
                .all(|(axis, (cached, incoming))| axis == 2 || cached == incoming)
    }

    /// (Re)allocate the buffers, carrying over what is already cached. Runs on
    /// the first append of a stack and whenever the batch shape changes; a
    /// capacity bump only happens if the caller under-sized the cache.
    fn allocate(&mut self, k: &Tensor, v: &Tensor, seq: usize) -> Result<()> {
        let keep = if self.matches(k) { self.len } else { 0 };
        self.capacity = self.capacity.max(keep + seq);
        let mut shape = k.dims().to_vec();
        shape[2] = self.capacity;
        let keys = Tensor::zeros(shape.clone(), k.dtype(), k.device())?;
        let values = Tensor::zeros(shape, v.dtype(), v.device())?;
        if keep > 0
            && let Some((old_keys, old_values)) = &self.buffers
        {
            keys.slice_set(&old_keys.narrow(2, 0, keep)?.contiguous()?, 2, 0)?;
            values.slice_set(&old_values.narrow(2, 0, keep)?.contiguous()?, 2, 0)?;
        }
        self.buffers = Some((keys, values));
        self.len = keep;
        Ok(())
    }

    fn reset(&mut self) {
        self.len = 0;
    }
}

pub struct StackCache {
    layers: Vec<LayerKvCache>,
}

impl StackCache {
    pub fn new(num_layers: usize, capacity: usize) -> Self {
        Self {
            layers: (0..num_layers)
                .map(|_| LayerKvCache::new(capacity))
                .collect(),
        }
    }

    pub fn layer(&mut self, index: usize) -> &mut LayerKvCache {
        &mut self.layers[index]
    }

    /// Number of cached positions (identical across layers).
    pub fn len(&self) -> usize {
        self.layers.first().map_or(0, |layer| layer.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rewind every layer to position zero, keeping the buffers allocated.
    pub fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::{DType, Device};

    fn step(value: f32, positions: usize) -> Tensor {
        Tensor::full(value, (1, 2, positions, 3), &Device::Cpu)
            .unwrap()
            .to_dtype(DType::F32)
            .unwrap()
    }

    #[test]
    fn append_grows_the_view_and_keeps_history() {
        let mut cache = LayerKvCache::new(4);
        let (k, _) = cache.append(&step(1.0, 2), &step(1.0, 2)).unwrap();
        assert_eq!(k.dims(), &[1, 2, 2, 3]);

        let (k, v) = cache.append(&step(2.0, 1), &step(2.0, 1)).unwrap();
        assert_eq!(k.dims(), &[1, 2, 3, 3]);
        let values: Vec<f32> = v.flatten_all().unwrap().to_vec1().unwrap();
        // Two positions of 1.0 followed by one of 2.0, per head.
        assert_eq!(values[0..6], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(values[6..9], [2.0, 2.0, 2.0]);
    }

    #[test]
    fn reset_rewinds_without_dropping_the_buffer() {
        let mut cache = LayerKvCache::new(4);
        cache.append(&step(1.0, 3), &step(1.0, 3)).unwrap();
        cache.reset();
        assert_eq!(cache.len, 0);
        assert!(cache.buffers.is_some());

        let (k, v) = cache.append(&step(5.0, 1), &step(5.0, 1)).unwrap();
        assert_eq!(k.dims(), &[1, 2, 1, 3]);
        let values: Vec<f32> = v.flatten_all().unwrap().to_vec1().unwrap();
        assert_eq!(values, vec![5.0; 6]);
    }

    #[test]
    fn a_batch_change_reallocates() {
        let mut cache = LayerKvCache::new(4);
        cache.append(&step(1.0, 1), &step(1.0, 1)).unwrap();
        let wide = Tensor::full(7.0f32, (2, 2, 1, 3), &Device::Cpu).unwrap();
        let (k, _) = cache.append(&wide, &wide).unwrap();
        assert_eq!(k.dims(), &[2, 2, 1, 3]);
    }

    /// The attention path feeds these views straight into matmul without a
    /// `contiguous` copy; guard that candle handles their strides.
    #[test]
    fn cache_views_matmul_like_contiguous_copies() {
        let mut cache = LayerKvCache::new(8);
        for position in 0..5 {
            let value = position as f32 + 1.0;
            cache
                .append(&step(value, 1), &step(value * 0.5, 1))
                .unwrap();
        }
        let (k, v) = cache.append(&step(9.0, 1), &step(4.5, 1)).unwrap();
        let q = Tensor::full(0.25f32, (1, 2, 1, 3), &Device::Cpu).unwrap();

        let scores = q.matmul(&k.transpose(2, 3).unwrap()).unwrap();
        let expected_scores = q
            .matmul(&k.transpose(2, 3).unwrap().contiguous().unwrap())
            .unwrap();
        assert_eq!(
            scores.flatten_all().unwrap().to_vec1::<f32>().unwrap(),
            expected_scores
                .flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap()
        );

        let out = scores.matmul(&v).unwrap();
        let expected_out = scores.matmul(&v.contiguous().unwrap()).unwrap();
        assert_eq!(
            out.flatten_all().unwrap().to_vec1::<f32>().unwrap(),
            expected_out
                .flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap()
        );
    }

    #[test]
    fn overflowing_capacity_preserves_the_cache() {
        let mut cache = LayerKvCache::new(2);
        cache.append(&step(1.0, 2), &step(1.0, 2)).unwrap();
        let (k, v) = cache.append(&step(3.0, 1), &step(3.0, 1)).unwrap();
        assert_eq!(k.dims(), &[1, 2, 3, 3]);
        let values: Vec<f32> = v.flatten_all().unwrap().to_vec1().unwrap();
        assert_eq!(values[0..6], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(values[6..9], [3.0, 3.0, 3.0]);
    }
}
