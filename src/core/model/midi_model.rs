//! The dual-Llama MIDI model: a base network over events, a token network that
//! decodes each event's sub-tokens, and a shared language-model head.

use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Linear, Module, VarBuilder};

use super::config::ModelConfig;
use super::kv_cache::StackCache;
use super::llama::{LlamaStack, last_position};
use super::weights;

pub struct MidiModel {
    base: LlamaStack,
    token: LlamaStack,
    lm_head: Linear,
    config: ModelConfig,
    device: Device,
    dtype: DType,
}

impl MidiModel {
    /// Load the checkpoint in `dtype`. f16 halves both the resident weights and
    /// the bytes read per decode step, which is what CPU generation is bound by.
    pub fn load(config: ModelConfig, weights: &[u8], device: Device, dtype: DType) -> Result<Self> {
        let tensors = weights::load_tensors(weights, &device, dtype)
            .map_err(|e| candle_core::Error::Msg(format!("loading weights: {e}")))?;
        let vb = VarBuilder::from_tensors(tensors, dtype, &device);
        let base = LlamaStack::new(&config.net, &device, vb.pp("net"))?;
        let token = LlamaStack::new(&config.net_token, &device, vb.pp("net_token"))?;
        let lm_head = candle_nn::linear_no_bias(
            config.net_token.hidden_size,
            config.net.vocab_size,
            vb.pp("lm_head"),
        )?;
        Ok(Self {
            base,
            token,
            lm_head,
            config,
            device,
            dtype,
        })
    }

    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Precision of the weights and, in turn, of the attention caches.
    pub fn dtype(&self) -> DType {
        self.dtype
    }

    /// Cache for the base net, preallocated for `capacity` events.
    pub fn base_cache(&self, capacity: usize) -> StackCache {
        StackCache::new(self.config.net.num_hidden_layers, capacity)
    }

    /// Cache for the token net, preallocated for `capacity` sub-tokens.
    pub fn token_cache(&self, capacity: usize) -> StackCache {
        StackCache::new(self.config.net_token.num_hidden_layers, capacity)
    }

    /// Base net over a run of events `ids` (b, seq, max_token_seq) of u32 sub-token
    /// ids. Event embedding is the sum over the sub-token axis (pad-inclusive).
    /// Returns the hidden state of the last event: (b, hidden).
    pub fn base_forward(&self, ids: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let embeds = self.base.embed(ids)?.sum(2)?;
        let hidden = self.base.forward(&embeds, cache)?;
        last_position(&hidden)
    }

    /// First token-net step: seed with the base hidden state (b, hidden).
    pub fn token_logits_from_hidden(
        &self,
        hidden: &Tensor,
        cache: &mut StackCache,
    ) -> Result<Tensor> {
        let embeds = hidden.unsqueeze(1)?;
        self.token_logits(&embeds, cache)
    }

    /// Subsequent token-net step: feed the embedding of the previously sampled id.
    pub fn token_logits_from_id(&self, ids: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let embeds = self.token.embed(ids)?;
        self.token_logits(&embeds, cache)
    }

    /// Logits come back as f32 whatever the weights are, so sampling stays
    /// identical across precisions.
    fn token_logits(&self, embeds: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let hidden = self.token.forward(embeds, cache)?;
        self.lm_head
            .forward(&last_position(&hidden)?)?
            .to_dtype(DType::F32)
    }
}
