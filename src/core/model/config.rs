//! Model hyperparameters parsed from the checkpoint's config.json.
//!
//! The two Llama stacks ("net" and "net_token") are stock Llama with no biases.
//! rms_norm_eps and rope_theta are not stored in config.json (transformers
//! defaults), so we keep them as constants.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::core::tokenizer::events::{EVENT_ORDER, FIELD_ORDER};
use crate::core::tokenizer::vocab::{BOS_ID, EOS_ID, MAX_TOKEN_SEQ, PAD_ID, VOCAB_SIZE};

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
    pub fn from_json(json: &str) -> std::result::Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn from_compatible_json(json: &str) -> Result<Self> {
        let checkpoint: CheckpointConfig =
            serde_json::from_str(json).context("parsing model config.json")?;
        checkpoint.validate()?;
        Ok(Self {
            net: checkpoint.net,
            net_token: checkpoint.net_token,
        })
    }
}

#[derive(Deserialize)]
struct CheckpointConfig {
    architectures: Vec<String>,
    model_type: String,
    #[serde(rename = "net_config")]
    net: LlamaConfig,
    #[serde(rename = "net_token_config")]
    net_token: LlamaConfig,
    tokenizer: TokenizerConfig,
}

#[derive(Deserialize)]
struct TokenizerConfig {
    bos_id: u32,
    eos_id: u32,
    pad_id: u32,
    version: String,
    optimise_midi: bool,
    max_token_seq: usize,
    vocab_size: u32,
    event_parameters: HashMap<String, u32>,
    events: HashMap<String, Vec<String>>,
}

impl CheckpointConfig {
    fn validate(&self) -> Result<()> {
        if self.model_type != "midi_model"
            || !self.architectures.iter().any(|name| name == "MIDIModel")
        {
            bail!("the checkpoint is not a MIDIModel");
        }
        validate_llama("base network", &self.net)?;
        validate_llama("token network", &self.net_token)?;
        if self.net.hidden_size != self.net_token.hidden_size {
            bail!("base and token networks must use the same hidden size");
        }
        if self.net.vocab_size != VOCAB_SIZE as usize
            || self.net_token.vocab_size != VOCAB_SIZE as usize
        {
            bail!("the checkpoint vocabulary is not compatible with tokenizer v2");
        }
        self.tokenizer.validate()
    }
}

fn validate_llama(name: &str, config: &LlamaConfig) -> Result<()> {
    if config.hidden_size == 0
        || config.intermediate_size == 0
        || config.num_hidden_layers == 0
        || config.num_attention_heads == 0
        || config.num_key_value_heads == 0
        || config.hidden_size % config.num_attention_heads != 0
        || config.num_attention_heads % config.num_key_value_heads != 0
    {
        bail!("{name} has invalid dimensions");
    }
    if config.max_position_embeddings < 4096 {
        bail!("{name} does not support the application's 4096-event context");
    }
    if config.pad_token_id != PAD_ID {
        bail!("{name} uses an incompatible padding token");
    }
    Ok(())
}

impl TokenizerConfig {
    fn validate(&self) -> Result<()> {
        if self.version != "v2"
            || !self.optimise_midi
            || self.bos_id != BOS_ID
            || self.eos_id != EOS_ID
            || self.pad_id != PAD_ID
            || self.max_token_seq != MAX_TOKEN_SEQ
            || self.vocab_size != VOCAB_SIZE
        {
            bail!("the checkpoint tokenizer metadata is not compatible with tv2o");
        }
        if self.event_parameters.len() != FIELD_ORDER.len()
            || FIELD_ORDER
                .iter()
                .any(|field| self.event_parameters.get(field.name()).copied() != Some(field.size()))
        {
            bail!("the checkpoint tokenizer field layout is not compatible with tv2o");
        }
        if self.events.len() != EVENT_ORDER.len()
            || EVENT_ORDER.iter().any(|event| {
                let expected: Vec<&str> = event.fields().iter().map(|field| field.name()).collect();
                self.events
                    .get(event.name())
                    .is_none_or(|actual| actual.iter().map(String::as_str).ne(expected))
            })
        {
            bail!("the checkpoint tokenizer event layout is not compatible with tv2o");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checkpoint_config() {
        let config =
            ModelConfig::from_compatible_json(crate::services::model_catalog::DEFAULT_CONFIG_JSON)
                .unwrap();
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

    #[test]
    fn rejects_an_incompatible_tokenizer() {
        let mut config: serde_json::Value =
            serde_json::from_str(crate::services::model_catalog::DEFAULT_CONFIG_JSON).unwrap();
        config["tokenizer"]["version"] = "v1".into();
        assert!(ModelConfig::from_compatible_json(&config.to_string()).is_err());
    }
}
