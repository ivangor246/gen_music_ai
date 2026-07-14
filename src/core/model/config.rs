//! Model hyperparameters parsed from the checkpoint's config.json.
//!
//! The two Llama stacks ("net" and "net_token") are stock Llama with no biases.
//! rms_norm_eps and rope_theta are not stored in config.json (transformers
//! defaults), so we keep them as constants.

use serde::Deserialize;

pub const RMS_NORM_EPS: f64 = 1e-6;
pub const ROPE_THETA: f64 = 10_000.0;

#[derive(Debug, Clone, Deserialize)]
pub struct LlamaConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub max_position_embeddings: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub vocab_size: usize,
    pub pad_token_id: u32,
}

impl LlamaConfig {
    pub fn head_dim(&self) -> usize {
        self.hidden_size / self.num_attention_heads
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    #[serde(rename = "net_config")]
    pub net: LlamaConfig,
    #[serde(rename = "net_token_config")]
    pub net_token: LlamaConfig,
}

impl ModelConfig {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checkpoint_config() {
        let config = ModelConfig::from_json(crate::assets::CONFIG_JSON).unwrap();
        assert_eq!(config.net.hidden_size, 1024);
        assert_eq!(config.net.num_hidden_layers, 12);
        assert_eq!(config.net.num_attention_heads, 16);
        assert_eq!(config.net.head_dim(), 64);
        assert_eq!(config.net.intermediate_size, 4096);
        assert_eq!(config.net.vocab_size, 3406);

        assert_eq!(config.net_token.hidden_size, 1024);
        assert_eq!(config.net_token.num_hidden_layers, 3);
        assert_eq!(config.net_token.num_attention_heads, 4);
        assert_eq!(config.net_token.head_dim(), 256);
        assert_eq!(config.net_token.intermediate_size, 1024);
    }
}
