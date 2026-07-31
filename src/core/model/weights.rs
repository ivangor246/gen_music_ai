//! Load model weights from the (embedded or dev) safetensors into candle tensors.
//!
//! The checkpoint stores bf16 tensors with a plain HF-Llama key layout
//! (`net.*`, `net_token.*`, `lm_head.weight`). We convert bf16 -> f32 once at
//! load time (candle CPU compute is fastest in f32). The safetensors header is
//! parsed by borrowing the asset bytes, so no owning copy of the 447MB is made
//! before the single per-tensor conversion.

use std::collections::HashMap;

use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use half::bf16;
use safetensors::SafeTensors;
use safetensors::tensor::{Dtype, TensorView};

use crate::assets;

/// Load every tensor as f32 on the given device, keyed by its checkpoint name.
pub fn load_tensors(device: &Device) -> Result<HashMap<String, Tensor>> {
    let bytes = assets::model_safetensors()?;
    let safetensors =
        SafeTensors::deserialize(bytes.as_ref()).context("parsing model.safetensors")?;
    let mut tensors = HashMap::new();
    for (name, view) in safetensors.tensors() {
        let tensor =
            view_to_f32(&view, device).with_context(|| format!("loading tensor `{name}`"))?;
        tensors.insert(name, tensor);
    }
    Ok(tensors)
}

fn view_to_f32(view: &TensorView, device: &Device) -> Result<Tensor> {
    let shape = view.shape().to_vec();
    let data = view.data();
    let values: Vec<f32> = match view.dtype() {
        Dtype::BF16 => data
            .chunks_exact(2)
            .map(|b| bf16::from_bits(u16::from_le_bytes([b[0], b[1]])).to_f32())
            .collect(),
        Dtype::F32 => data
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        other => anyhow::bail!("unsupported tensor dtype {other:?}"),
    };
    Ok(Tensor::from_vec(values, shape, device)?)
}

#[cfg(all(test, feature = "heavy-tests"))]
mod tests {
    use super::*;

    #[test]
    fn loads_expected_tensors() {
        let _guard = super::super::HEAVY_TEST_LOCK.lock().unwrap();
        let device = Device::Cpu;
        let tensors = load_tensors(&device).unwrap();

        // 110 base-net + 29 token-net + lm_head = 140 tensors.
        assert_eq!(tensors.len(), 140);

        let checks = [
            ("lm_head.weight", vec![3406, 1024]),
            ("net.embed_tokens.weight", vec![3406, 1024]),
            ("net.layers.0.self_attn.q_proj.weight", vec![1024, 1024]),
            ("net.layers.0.mlp.gate_proj.weight", vec![4096, 1024]),
            ("net.layers.0.mlp.down_proj.weight", vec![1024, 4096]),
            ("net.norm.weight", vec![1024]),
            ("net_token.embed_tokens.weight", vec![3406, 1024]),
            ("net_token.layers.0.mlp.gate_proj.weight", vec![1024, 1024]),
            ("net_token.norm.weight", vec![1024]),
        ];
        for (name, shape) in checks {
            let tensor = tensors
                .get(name)
                .unwrap_or_else(|| panic!("missing tensor {name}"));
            assert_eq!(tensor.dims(), shape.as_slice(), "shape of {name}");
        }
    }
}
