//! A minimal Llama stack (RMSNorm pre-norm, RoPE attention, SwiGLU MLP), shared
//! by both the base net and the token net. No biases, no GQA (kv heads == heads
//! for both configs). Weights are pulled from a `VarBuilder` with the HF layout.

use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Embedding, Linear, Module, RmsNorm, VarBuilder};

use super::config::{LlamaConfig, RMS_NORM_EPS, ROPE_THETA};
use super::kv_cache::{LayerKvCache, StackCache};
use super::rope::RotaryCache;

struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl Attention {
    fn new(config: &LlamaConfig, vb: VarBuilder) -> Result<Self> {
        let hidden = config.hidden_size;
        let head_dim = config.head_dim();
        Ok(Self {
            q_proj: candle_nn::linear_no_bias(hidden, hidden, vb.pp("q_proj"))?,
            k_proj: candle_nn::linear_no_bias(hidden, hidden, vb.pp("k_proj"))?,
            v_proj: candle_nn::linear_no_bias(hidden, hidden, vb.pp("v_proj"))?,
            o_proj: candle_nn::linear_no_bias(hidden, hidden, vb.pp("o_proj"))?,
            num_heads: config.num_attention_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn shape_heads(&self, x: &Tensor, b: usize, seq: usize) -> Result<Tensor> {
        x.reshape((b, seq, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()
    }

    fn forward(
        &self,
        x: &Tensor,
        rope: &RotaryCache,
        cache: &mut LayerKvCache,
        offset: usize,
        mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (b, seq, hidden) = x.dims3()?;
        let q = self.shape_heads(&self.q_proj.forward(x)?, b, seq)?;
        let k = self.shape_heads(&self.k_proj.forward(x)?, b, seq)?;
        let v = self.shape_heads(&self.v_proj.forward(x)?, b, seq)?;

        let q = rope.apply(&q, offset)?;
        let k = rope.apply(&k, offset)?;
        // Both come back as views over the layer's cache buffers; candle's cpu
        // matmul reads their strides directly, so no `contiguous` copy is needed.
        let (k, v) = cache.append(&k, &v)?;

        let scores = (q.matmul(&k.transpose(2, 3)?)? * self.scale)?;
        let scores = match mask {
            Some(mask) => scores.broadcast_add(mask)?,
            None => scores,
        };
        let weights = candle_nn::ops::softmax_last_dim(&scores)?;

        let out = weights.matmul(&v)?;
        let out = out.transpose(1, 2)?.reshape((b, seq, hidden))?;
        self.o_proj.forward(&out)
    }
}

/// Additive causal mask (seq, total): query i is at absolute position offset+i.
fn causal_mask(
    seq: usize,
    total: usize,
    offset: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let mut data = vec![0f32; seq * total];
    for i in 0..seq {
        let limit = offset + i;
        for j in 0..total {
            if j > limit {
                data[i * total + j] = f32::NEG_INFINITY;
            }
        }
    }
    Tensor::from_vec(data, (seq, total), device)?.to_dtype(dtype)
}

struct Mlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
}

impl Mlp {
    fn new(config: &LlamaConfig, vb: VarBuilder) -> Result<Self> {
        let hidden = config.hidden_size;
        let inter = config.intermediate_size;
        Ok(Self {
            gate_proj: candle_nn::linear_no_bias(hidden, inter, vb.pp("gate_proj"))?,
            up_proj: candle_nn::linear_no_bias(hidden, inter, vb.pp("up_proj"))?,
            down_proj: candle_nn::linear_no_bias(inter, hidden, vb.pp("down_proj"))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let gate = candle_nn::ops::silu(&self.gate_proj.forward(x)?)?;
        let up = self.up_proj.forward(x)?;
        self.down_proj.forward(&(gate * up)?)
    }
}

struct Block {
    input_layernorm: RmsNorm,
    attn: Attention,
    post_attention_layernorm: RmsNorm,
    mlp: Mlp,
}

impl Block {
    fn new(config: &LlamaConfig, vb: VarBuilder) -> Result<Self> {
        let hidden = config.hidden_size;
        Ok(Self {
            input_layernorm: candle_nn::rms_norm(hidden, RMS_NORM_EPS, vb.pp("input_layernorm"))?,
            attn: Attention::new(config, vb.pp("self_attn"))?,
            post_attention_layernorm: candle_nn::rms_norm(
                hidden,
                RMS_NORM_EPS,
                vb.pp("post_attention_layernorm"),
            )?,
            mlp: Mlp::new(config, vb.pp("mlp"))?,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        rope: &RotaryCache,
        cache: &mut LayerKvCache,
        offset: usize,
        mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let residual = x;
        let hidden = self.input_layernorm.forward(x)?;
        let hidden = self.attn.forward(&hidden, rope, cache, offset, mask)?;
        let x = (residual + hidden)?;
        let residual = &x;
        let hidden = self.post_attention_layernorm.forward(&x)?;
        let hidden = self.mlp.forward(&hidden)?;
        residual + hidden
    }
}

pub struct LlamaStack {
    embed_tokens: Embedding,
    blocks: Vec<Block>,
    norm: RmsNorm,
    rope: RotaryCache,
}

impl LlamaStack {
    pub fn new(config: &LlamaConfig, device: &Device, vb: VarBuilder) -> Result<Self> {
        let embed_tokens =
            candle_nn::embedding(config.vocab_size, config.hidden_size, vb.pp("embed_tokens"))?;
        let mut blocks = Vec::with_capacity(config.num_hidden_layers);
        for layer in 0..config.num_hidden_layers {
            blocks.push(Block::new(config, vb.pp(format!("layers.{layer}")))?);
        }
        let norm = candle_nn::rms_norm(config.hidden_size, RMS_NORM_EPS, vb.pp("norm"))?;
        let rope = RotaryCache::new(
            config.head_dim(),
            config.max_position_embeddings,
            ROPE_THETA,
            vb.dtype(),
            device,
        )?;
        Ok(Self {
            embed_tokens,
            blocks,
            norm,
            rope,
        })
    }

    /// Look up token embeddings for `ids` (appends a trailing hidden dim).
    pub fn embed(&self, ids: &Tensor) -> Result<Tensor> {
        self.embed_tokens.forward(ids)
    }

    /// Run the blocks + final norm over input embeddings (b, seq, hidden). RoPE
    /// positions start at the current cache length.
    pub fn forward(&self, embeds: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let offset = cache.len();
        let seq = embeds.dim(1)?;
        // Every layer of the stack shares one mask, so build it once here. A
        // single query attends to the whole cache, making the mask all zeros --
        // skip it entirely on the decode path.
        let mask = if seq > 1 {
            Some(causal_mask(
                seq,
                offset + seq,
                offset,
                embeds.dtype(),
                embeds.device(),
            )?)
        } else {
            None
        };
        let mut x = embeds.clone();
        for (index, block) in self.blocks.iter().enumerate() {
            x = block.forward(&x, &self.rope, cache.layer(index), offset, mask.as_ref())?;
        }
        self.norm.forward(&x)
    }
}

/// Take the last position along the sequence axis: (b, seq, hidden) -> (b, hidden).
pub fn last_position(hidden: &Tensor) -> Result<Tensor> {
    let seq = hidden.dim(1)?;
    hidden.narrow(1, seq - 1, 1)?.squeeze(1)
}
