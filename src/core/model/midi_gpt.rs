//! MIDI-GPT Yellow configuration and GPT-2 inference model.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result as AnyResult, bail};
use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Embedding, LayerNorm, Linear, Module, VarBuilder};
use serde::Deserialize;

use super::kv_cache::{LayerKvCache, StackCache};
use super::weights;

const EMBEDDING_SIZE: usize = 512;
const LAYER_COUNT: usize = 6;
const HEAD_COUNT: usize = 8;
pub const MIDI_GPT_MAX_POSITIONS: usize = 2048;
const LAYER_NORM_EPS: f64 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiGptToken {
    PieceStart,
    Track,
    TrackEnd,
    Bar,
    BarEnd,
    TimeSignature,
    Instrument,
    NumBars,
    NoteOnset,
    NoteDuration,
    TimeAbsolutePosition,
    VelocityLevel,
    FillInPlaceholder,
    FillInStart,
    FillInEnd,
    NoteDensity,
    MinPolyphony,
    MaxPolyphony,
    MinNoteDuration,
    MaxNoteDuration,
}

const YELLOW_DOMAINS: [(MidiGptToken, u32); 20] = [
    (MidiGptToken::PieceStart, 2),
    (MidiGptToken::Track, 2),
    (MidiGptToken::TrackEnd, 1),
    (MidiGptToken::Bar, 1),
    (MidiGptToken::BarEnd, 1),
    (MidiGptToken::TimeSignature, 36),
    (MidiGptToken::Instrument, 109),
    (MidiGptToken::NumBars, 2),
    (MidiGptToken::NoteOnset, 128),
    (MidiGptToken::NoteDuration, 96),
    (MidiGptToken::TimeAbsolutePosition, 192),
    (MidiGptToken::VelocityLevel, 32),
    (MidiGptToken::FillInPlaceholder, 1),
    (MidiGptToken::FillInStart, 1),
    (MidiGptToken::FillInEnd, 1),
    (MidiGptToken::NoteDensity, 10),
    (MidiGptToken::MinPolyphony, 10),
    (MidiGptToken::MaxPolyphony, 10),
    (MidiGptToken::MinNoteDuration, 6),
    (MidiGptToken::MaxNoteDuration, 6),
];

impl MidiGptToken {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "PieceStart" => Self::PieceStart,
            "Track" => Self::Track,
            "TrackEnd" => Self::TrackEnd,
            "Bar" => Self::Bar,
            "BarEnd" => Self::BarEnd,
            "TimeSig" => Self::TimeSignature,
            "Instrument" => Self::Instrument,
            "NumBars" => Self::NumBars,
            "NoteOnset" => Self::NoteOnset,
            "NoteDuration" => Self::NoteDuration,
            "TimeAbsolutePos" => Self::TimeAbsolutePosition,
            "VelocityLevel" => Self::VelocityLevel,
            "FillInPlaceholder" => Self::FillInPlaceholder,
            "FillInStart" => Self::FillInStart,
            "FillInEnd" => Self::FillInEnd,
            "NoteDensity" => Self::NoteDensity,
            "MinPolyphony" => Self::MinPolyphony,
            "MaxPolyphony" => Self::MaxPolyphony,
            "MinNoteDuration" => Self::MinNoteDuration,
            "MaxNoteDuration" => Self::MaxNoteDuration,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
struct Domain {
    kind: MidiGptToken,
    offset: u32,
    size: u32,
}

#[derive(Debug, Clone)]
pub struct MidiGptVocabulary {
    domains: Vec<Domain>,
    size: usize,
}

impl MidiGptVocabulary {
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn encode(&self, kind: MidiGptToken, value: u32) -> Option<u32> {
        let domain = self.domains.iter().find(|domain| domain.kind == kind)?;
        (value < domain.size).then_some(domain.offset + value)
    }

    pub fn decode(&self, id: u32) -> Option<(MidiGptToken, u32)> {
        self.domains
            .iter()
            .find(|domain| id >= domain.offset && id < domain.offset + domain.size)
            .map(|domain| (domain.kind, id - domain.offset))
    }

    pub fn ids(&self, kind: MidiGptToken) -> Vec<u32> {
        self.domains
            .iter()
            .find(|domain| domain.kind == kind)
            .map(|domain| (domain.offset..domain.offset + domain.size).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct MidiGptConfig {
    pub resolution: u32,
    pub decode_resolution: u32,
    pub time_signatures: Vec<(u32, u32)>,
    pub num_bars: Vec<u32>,
    pub vocabulary: MidiGptVocabulary,
    instrument_to_group: [u8; 128],
    group_to_instrument: Vec<u8>,
    velocity_decode: Vec<u8>,
}

#[derive(Deserialize)]
struct RawConfig {
    resolution: u32,
    decode_resolution: u32,
    time_signatures: Vec<String>,
    num_bars_map: Vec<u32>,
    instrument_merge_groups: Vec<Vec<u8>>,
    velocity_levels: usize,
    token_domains: Vec<RawDomain>,
}

#[derive(Deserialize)]
struct RawDomain {
    domain_size: u32,
    #[serde(rename = "type")]
    name: String,
}

impl MidiGptConfig {
    pub fn from_json(json: &str) -> AnyResult<Self> {
        let raw: RawConfig = serde_json::from_str(json).context("parsing MIDI-GPT encoder")?;
        let mut offset = 0u32;
        let mut domains = Vec::with_capacity(raw.token_domains.len());
        let mut kinds = HashSet::new();
        for domain in raw.token_domains {
            let kind = MidiGptToken::from_name(&domain.name)
                .with_context(|| format!("unsupported MIDI-GPT token domain `{}`", domain.name))?;
            if domain.domain_size == 0 || !kinds.insert(kind) {
                bail!("invalid MIDI-GPT token domain `{}`", domain.name);
            }
            domains.push(Domain {
                kind,
                offset,
                size: domain.domain_size,
            });
            offset += domain.domain_size;
        }
        let layout_matches = domains
            .iter()
            .zip(YELLOW_DOMAINS)
            .all(|(domain, expected)| (domain.kind, domain.size) == expected)
            && domains.len() == YELLOW_DOMAINS.len();
        if !layout_matches
            || offset != 647
            || raw.resolution != 12
            || raw.decode_resolution != 1920
            || raw.velocity_levels != 32
            || !raw.num_bars_map.contains(&4)
        {
            bail!("the encoder is not compatible with MIDI-GPT Yellow");
        }
        let time_signatures = raw
            .time_signatures
            .iter()
            .map(|signature| parse_signature(signature))
            .collect::<AnyResult<Vec<_>>>()?;
        let (instrument_to_group, group_to_instrument) =
            instrument_groups(&raw.instrument_merge_groups)?;
        Ok(Self {
            resolution: raw.resolution,
            decode_resolution: raw.decode_resolution,
            time_signatures,
            num_bars: raw.num_bars_map,
            vocabulary: MidiGptVocabulary {
                domains,
                size: offset as usize,
            },
            instrument_to_group,
            group_to_instrument,
            velocity_decode: velocity_decode_table(raw.velocity_levels),
        })
    }

    pub fn time_signature_value(&self, numerator: u32, denominator: u32) -> Option<u32> {
        self.time_signatures
            .iter()
            .position(|signature| *signature == (numerator, denominator))
            .map(|index| index as u32)
    }

    pub fn instrument_value(&self, program: u8) -> u32 {
        self.instrument_to_group[program as usize] as u32
    }

    pub fn instrument_program(&self, value: u32) -> u8 {
        self.group_to_instrument
            .get(value as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn velocity(&self, level: u32) -> u8 {
        self.velocity_decode
            .get(level as usize)
            .copied()
            .filter(|velocity| *velocity > 0)
            .unwrap_or(100)
    }

    pub fn velocity_level(&self, velocity: u8) -> u32 {
        self.velocity_decode
            .iter()
            .enumerate()
            .skip(1)
            .min_by_key(|(_, candidate)| candidate.abs_diff(velocity))
            .map(|(level, _)| level as u32)
            .unwrap_or(1)
    }
}

fn parse_signature(value: &str) -> AnyResult<(u32, u32)> {
    let (numerator, denominator) = value
        .split_once('/')
        .with_context(|| format!("invalid time signature `{value}`"))?;
    Ok((numerator.parse()?, denominator.parse()?))
}

fn instrument_groups(groups: &[Vec<u8>]) -> AnyResult<([u8; 128], Vec<u8>)> {
    let mut representative = [0u8; 128];
    for (index, value) in representative.iter_mut().enumerate() {
        *value = index as u8;
    }
    for group in groups {
        let Some(&first) = group.first() else {
            continue;
        };
        for &program in group {
            representative[program as usize] = first;
        }
    }
    let mut representatives = representative.to_vec();
    representatives.sort_unstable();
    representatives.dedup();
    let dense: HashMap<u8, u8> = representatives
        .iter()
        .enumerate()
        .map(|(index, &program)| (program, index as u8))
        .collect();
    let mut forward = [0u8; 128];
    for (program, value) in forward.iter_mut().enumerate() {
        *value = *dense
            .get(&representative[program])
            .context("building MIDI-GPT instrument groups")?;
    }
    Ok((forward, representatives))
}

fn velocity_decode_table(levels: usize) -> Vec<u8> {
    let mut table = vec![0u8; levels];
    let mut counts = vec![0u8; levels];
    for velocity in 1..128u8 {
        let level = (1 + usize::from(velocity) * (levels - 1) / 128).min(levels - 1);
        counts[level] += 1;
        if counts[level] == 2 {
            table[level] = velocity;
        }
    }
    table
}

struct Conv1d {
    weight: Tensor,
    bias: Tensor,
}

impl Conv1d {
    fn new(input: usize, output: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            weight: vb.get((input, output), "weight")?,
            bias: vb.get(output, "bias")?,
        })
    }

    fn forward(&self, input: &Tensor) -> Result<Tensor> {
        let (batch, sequence, width) = input.dims3()?;
        input
            .reshape((batch * sequence, width))?
            .matmul(&self.weight)?
            .broadcast_add(&self.bias)?
            .reshape((batch, sequence, ()))
    }
}

struct Attention {
    qkv: Conv1d,
    output: Conv1d,
    head_dim: usize,
}

impl Attention {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            qkv: Conv1d::new(EMBEDDING_SIZE, 3 * EMBEDDING_SIZE, vb.pp("c_attn"))?,
            output: Conv1d::new(EMBEDDING_SIZE, EMBEDDING_SIZE, vb.pp("c_proj"))?,
            head_dim: EMBEDDING_SIZE / HEAD_COUNT,
        })
    }

    fn forward(&self, input: &Tensor, cache: &mut LayerKvCache, offset: usize) -> Result<Tensor> {
        let (batch, sequence, _) = input.dims3()?;
        let qkv = self.qkv.forward(input)?;
        let shape = (batch, sequence, HEAD_COUNT, self.head_dim);
        let split = |start| -> Result<Tensor> {
            qkv.narrow(2, start, EMBEDDING_SIZE)?
                .reshape(shape)?
                .transpose(1, 2)?
                .contiguous()
        };
        let query = split(0)?;
        let key = split(EMBEDDING_SIZE)?;
        let value = split(2 * EMBEDDING_SIZE)?;
        let (key, value) = cache.append(&key, &value)?;
        let scores =
            (query.matmul(&key.transpose(2, 3)?)? * (1.0 / (self.head_dim as f64).sqrt()))?;
        let scores = if sequence > 1 {
            scores.broadcast_add(&causal_mask(
                sequence,
                offset + sequence,
                offset,
                scores.dtype(),
                scores.device(),
            )?)?
        } else {
            scores
        };
        let output = candle_nn::ops::softmax_last_dim(&scores)?
            .matmul(&value)?
            .transpose(1, 2)?
            .reshape((batch, sequence, EMBEDDING_SIZE))?;
        self.output.forward(&output)
    }
}

struct Mlp {
    input: Conv1d,
    output: Conv1d,
}

impl Mlp {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            input: Conv1d::new(EMBEDDING_SIZE, 4 * EMBEDDING_SIZE, vb.pp("c_fc"))?,
            output: Conv1d::new(4 * EMBEDDING_SIZE, EMBEDDING_SIZE, vb.pp("c_proj"))?,
        })
    }

    fn forward(&self, input: &Tensor) -> Result<Tensor> {
        self.output.forward(&gelu_new(&self.input.forward(input)?)?)
    }
}

struct Block {
    attention_norm: LayerNorm,
    attention: Attention,
    mlp_norm: LayerNorm,
    mlp: Mlp,
}

impl Block {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            attention_norm: candle_nn::layer_norm(EMBEDDING_SIZE, LAYER_NORM_EPS, vb.pp("ln_1"))?,
            attention: Attention::new(vb.pp("attn"))?,
            mlp_norm: candle_nn::layer_norm(EMBEDDING_SIZE, LAYER_NORM_EPS, vb.pp("ln_2"))?,
            mlp: Mlp::new(vb.pp("mlp"))?,
        })
    }

    fn forward(&self, input: &Tensor, cache: &mut LayerKvCache, offset: usize) -> Result<Tensor> {
        let attention =
            self.attention
                .forward(&self.attention_norm.forward(input)?, cache, offset)?;
        let hidden = (input + attention)?;
        let mlp = self.mlp.forward(&self.mlp_norm.forward(&hidden)?)?;
        hidden + mlp
    }
}

pub struct MidiGptModel {
    token_embedding: Embedding,
    position_embedding: Embedding,
    blocks: Vec<Block>,
    final_norm: LayerNorm,
    lm_head: Linear,
    config: MidiGptConfig,
    device: Device,
    dtype: DType,
}

impl MidiGptModel {
    pub fn load(
        config: MidiGptConfig,
        weights_bytes: &[u8],
        device: Device,
        dtype: DType,
    ) -> Result<Self> {
        let tensors = weights::load_tensors(weights_bytes, &device, dtype).map_err(|error| {
            candle_core::Error::Msg(format!("loading MIDI-GPT weights: {error}"))
        })?;
        let vb = VarBuilder::from_tensors(tensors, dtype, &device);
        let transformer = vb.pp("transformer");
        let token_embedding = candle_nn::embedding(
            config.vocabulary.size(),
            EMBEDDING_SIZE,
            transformer.pp("wte"),
        )?;
        let position_embedding = candle_nn::embedding(
            MIDI_GPT_MAX_POSITIONS,
            EMBEDDING_SIZE,
            transformer.pp("wpe"),
        )?;
        let mut blocks = Vec::with_capacity(LAYER_COUNT);
        for index in 0..LAYER_COUNT {
            blocks.push(Block::new(transformer.pp(format!("h.{index}")))?);
        }
        let final_norm =
            candle_nn::layer_norm(EMBEDDING_SIZE, LAYER_NORM_EPS, transformer.pp("ln_f"))?;
        let lm_head =
            candle_nn::linear_no_bias(EMBEDDING_SIZE, config.vocabulary.size(), vb.pp("lm_head"))?;
        Ok(Self {
            token_embedding,
            position_embedding,
            blocks,
            final_norm,
            lm_head,
            config,
            device,
            dtype,
        })
    }

    pub fn config(&self) -> &MidiGptConfig {
        &self.config
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn dtype(&self) -> DType {
        self.dtype
    }

    pub fn cache(&self) -> StackCache {
        StackCache::new(LAYER_COUNT, MIDI_GPT_MAX_POSITIONS)
    }

    pub fn forward(&self, ids: &Tensor, cache: &mut StackCache) -> Result<Tensor> {
        let offset = cache.len();
        let sequence = ids.dim(1)?;
        if offset + sequence > MIDI_GPT_MAX_POSITIONS {
            candle_core::bail!("MIDI-GPT context exceeds {MIDI_GPT_MAX_POSITIONS} tokens");
        }
        let positions: Vec<u32> = (offset as u32..(offset + sequence) as u32).collect();
        let positions = Tensor::from_vec(positions, (1, sequence), &self.device)?;
        let mut hidden = self
            .token_embedding
            .forward(ids)?
            .broadcast_add(&self.position_embedding.forward(&positions)?)?;
        for (index, block) in self.blocks.iter().enumerate() {
            hidden = block.forward(&hidden, cache.layer(index), offset)?;
        }
        let hidden = self.final_norm.forward(&hidden)?;
        let logits = self.lm_head.forward(&hidden)?.to_dtype(DType::F32)?;
        logits.narrow(1, sequence - 1, 1)?.squeeze(1)
    }
}

fn gelu_new(input: &Tensor) -> Result<Tensor> {
    let cube = (input.sqr()? * input)?;
    let inner = (input + (cube * 0.044715)?)?;
    let tanh = (inner * (2.0f64 / std::f64::consts::PI).sqrt())?.tanh()?;
    input * ((tanh + 1.0)? * 0.5)?
}

fn causal_mask(
    sequence: usize,
    total: usize,
    offset: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let mut values = vec![0f32; sequence * total];
    for query in 0..sequence {
        for key in offset + query + 1..total {
            values[query * total + key] = f32::NEG_INFINITY;
        }
    }
    Tensor::from_vec(values, (sequence, total), device)?.to_dtype(dtype)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YELLOW_CONFIG: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/models/midi-gpt-yellow/encoder.json"
    ));

    #[test]
    fn yellow_vocabulary_matches_checkpoint_layout() {
        let config = MidiGptConfig::from_json(YELLOW_CONFIG).unwrap();
        assert_eq!(config.vocabulary.size(), 647);
        assert_eq!(config.vocabulary.encode(MidiGptToken::Track, 0), Some(2));
        assert_eq!(
            config.vocabulary.encode(MidiGptToken::Instrument, 0),
            Some(43)
        );
        assert_eq!(
            config.vocabulary.encode(MidiGptToken::NoteOnset, 127),
            Some(281)
        );
    }

    #[test]
    fn merged_instruments_roundtrip_to_group_representative() {
        let config = MidiGptConfig::from_json(YELLOW_CONFIG).unwrap();
        assert_eq!(config.instrument_value(0), config.instrument_value(2));
        assert_eq!(config.instrument_program(config.instrument_value(2)), 0);
        assert_ne!(config.instrument_value(3), config.instrument_value(2));
    }

    #[test]
    fn changed_domain_order_is_rejected() {
        let mut config: serde_json::Value = serde_json::from_str(YELLOW_CONFIG).unwrap();
        config["token_domains"].as_array_mut().unwrap().swap(0, 1);
        assert!(MidiGptConfig::from_json(&config.to_string()).is_err());
    }
}
