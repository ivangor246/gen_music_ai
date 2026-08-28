//! Load model weights from verified safetensors bytes into candle tensors.
//!
//! The checkpoint stores bf16 tensors with a plain HF-Llama key layout
//! (`net.*`, `net_token.*`, `lm_head.weight`). We convert bf16 -> f32 once at
//! load time (candle CPU compute is fastest in f32). The safetensors header is
//! parsed by borrowing the asset bytes, so no owning copy of the 447MB is made
//! before the single per-tensor conversion.

use std::collections::HashMap;

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use half::{bf16, f16};
use safetensors::SafeTensors;
use safetensors::tensor::{Dtype, TensorView};

/// Load every tensor in `dtype` on the given device, keyed by its checkpoint
/// name. Conversion happens one tensor at a time, so the peak stays at the
/// asset plus the converted model rather than holding both dtypes in full.
pub fn load_tensors(
    bytes: &[u8],
    device: &Device,
    dtype: DType,
) -> Result<HashMap<String, Tensor>> {
    let safetensors = SafeTensors::deserialize(bytes).context("parsing model.safetensors")?;
    let mut tensors = HashMap::new();
    for (name, view) in safetensors.tensors() {
        let tensor = view_to_tensor(&view, dtype, device)
            .with_context(|| format!("loading tensor `{name}`"))?;
        tensors.insert(name, tensor);
    }
    Ok(tensors)
}

fn view_to_tensor(view: &TensorView, dtype: DType, device: &Device) -> Result<Tensor> {
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
    Ok(match dtype {
        DType::F16 => {
            let half: Vec<f16> = values.into_iter().map(f16::from_f32).collect();
            Tensor::from_vec(half, shape, device)?
        }
        _ => Tensor::from_vec(values, shape, device)?,
    })
}
