//! The dual-Llama MIDIModel: a base net over events, a token net that decodes
//! an event's sub-tokens, and a shared lm_head. Mirrors `MIDIModel.forward` /
//! `forward_token` in the Python model.

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
}

impl MidiModel {
    pub fn load(config: ModelConfig, device: Device) -> Result<Self> {
        let tensors = weights::load_tensors(&device)
            .map_err(|e| candle_core::Error::Msg(format!("loading weights: {e}")))?;
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &device);
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
        })
    }

    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn base_cache(&self) -> StackCache {
        StackCache::new(self.config.net.num_hidden_layers)
    }

    pub fn token_cache(&self) -> StackCache {
        StackCache::new(self.config.net_token.num_hidden_layers)
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

    fn token_logits(&self, embeds: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let hidden = self.token.forward(embeds, cache)?;
        self.lm_head.forward(&last_position(&hidden)?)
    }
}

#[cfg(all(test, feature = "heavy-tests"))]
mod tests {
    use super::*;
    use crate::assets;

    #[test]
    fn forward_produces_expected_shapes() {
        let _guard = super::super::HEAVY_TEST_LOCK.lock().unwrap();
        let config = ModelConfig::from_json(assets::CONFIG_JSON).unwrap();
        let device = Device::Cpu;
        let model = MidiModel::load(config, device.clone()).unwrap();

        // 1 batch, 2 events (bos row + a set_tempo-shaped row), 8 sub-tokens each.
        let ids = Tensor::from_vec(
            vec![
                1u32, 0, 0, 0, 0, 0, 0, 0, // bos
                6, 9, 137, 2201, 2985, 0, 0, 0, // set_tempo-ish
            ],
            (1, 2, 8),
            &device,
        )
        .unwrap();

        let mut base_cache = model.base_cache();
        let hidden = model.base_forward(&ids, &mut base_cache).unwrap();
        assert_eq!(hidden.dims(), &[1, 1024]);

        let mut token_cache = model.token_cache();
        let logits = model
            .token_logits_from_hidden(&hidden, &mut token_cache)
            .unwrap();
        assert_eq!(logits.dims(), &[1, 3406]);

        let prev = Tensor::from_vec(vec![6u32], (1, 1), &device).unwrap();
        let next = model.token_logits_from_id(&prev, &mut token_cache).unwrap();
        assert_eq!(next.dims(), &[1, 3406]);
    }
}
