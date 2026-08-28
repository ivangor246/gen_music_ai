//! Built-in catalog of model checkpoints supported by this application.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const TV2O_FORMAT: &str = "tv2o-safetensors-v1";
pub const MIDI_GPT_FORMAT: &str = "midi-gpt-yellow-safetensors-v2";
const CATALOG_SCHEMA_VERSION: u32 = 1;
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;
const MAX_WEIGHTS_SIZE: u64 = 16 * 1024 * 1024 * 1024;

const CATALOG_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/models/catalog.json"));

#[cfg(test)]
pub const DEFAULT_CONFIG_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/midi-model-tv2o-medium/config.json"
));

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelArtifact {
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub format: ModelFormat,
    pub source_url: String,
    pub license: String,
    pub config: ModelArtifact,
    pub weights: ModelArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelFormat {
    #[serde(rename = "tv2o-safetensors-v1")]
    Tv2o,
    #[serde(rename = "midi-gpt-yellow-safetensors-v2")]
    MidiGptYellow,
}

impl ModelFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tv2o => TV2O_FORMAT,
            Self::MidiGptYellow => MIDI_GPT_FORMAT,
        }
    }

    pub const fn max_tracks(self) -> usize {
        match self {
            Self::Tv2o => 15,
            Self::MidiGptYellow => 12,
        }
    }
}

impl std::fmt::Display for ModelFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ModelDescriptor {
    pub fn download_size(&self) -> u64 {
        self.config.size + self.weights.size
    }
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    models: Vec<ModelDescriptor>,
}

#[derive(Deserialize)]
struct CatalogFile {
    schema_version: u32,
    models: Vec<ModelDescriptor>,
}

impl ModelCatalog {
    pub fn load() -> Result<Self> {
        Self::from_json(CATALOG_JSON)
    }

    fn from_json(json: &str) -> Result<Self> {
        let catalog: CatalogFile =
            serde_json::from_str(json).context("parsing the built-in model catalog")?;
        if catalog.schema_version != CATALOG_SCHEMA_VERSION {
            bail!(
                "unsupported model catalog schema {}",
                catalog.schema_version
            );
        }
        if catalog.models.is_empty() {
            bail!("the model catalog is empty");
        }

        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for model in &catalog.models {
            validate_model(model)?;
            if !ids.insert(model.id.as_str()) {
                bail!("duplicate model id `{}`", model.id);
            }
            if !names.insert(model.name.as_str()) {
                bail!("duplicate model name `{}`", model.name);
            }
        }
        Ok(Self {
            models: catalog.models,
        })
    }

    pub fn models(&self) -> &[ModelDescriptor] {
        &self.models
    }

    pub fn find(&self, id: &str) -> Option<&ModelDescriptor> {
        self.models.iter().find(|model| model.id == id)
    }

    pub fn default_model(&self) -> &ModelDescriptor {
        &self.models[0]
    }
}

fn validate_model(model: &ModelDescriptor) -> Result<()> {
    if !valid_id(&model.id) {
        bail!("model id `{}` is not safe for local storage", model.id);
    }
    if model.name.trim().is_empty()
        || model.description.trim().is_empty()
        || model.license.trim().is_empty()
    {
        bail!("model `{}` has incomplete display metadata", model.id);
    }
    validate_https_url(&model.source_url)
        .with_context(|| format!("invalid source URL for `{}`", model.id))?;
    validate_artifact("config", &model.config, MAX_CONFIG_SIZE)
        .with_context(|| format!("invalid config artifact for `{}`", model.id))?;
    validate_artifact("weights", &model.weights, MAX_WEIGHTS_SIZE)
        .with_context(|| format!("invalid weights artifact for `{}`", model.id))
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    id.len() <= 80
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
}

fn validate_artifact(name: &str, artifact: &ModelArtifact, max_size: u64) -> Result<()> {
    validate_https_url(&artifact.url).with_context(|| format!("invalid {name} URL"))?;
    if artifact.size == 0 || artifact.size > max_size {
        bail!(
            "{name} size {} is outside the supported range",
            artifact.size
        );
    }
    if artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{name} SHA-256 must be 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn validate_https_url(value: &str) -> Result<()> {
    let url = Url::parse(value).context("parsing URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("only plain HTTPS URLs with a host are allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_catalog_is_valid() {
        let catalog = ModelCatalog::load().unwrap();
        let model = catalog.default_model();
        assert_eq!(model.id, "skytnt-midi-model-tv2o-medium");
        assert_eq!(model.weights.size, 467_701_064);
        assert_eq!(catalog.models().len(), 2);
        assert_eq!(catalog.models()[1].format, ModelFormat::MidiGptYellow);
    }

    #[test]
    fn unsafe_ids_and_urls_are_rejected() {
        let mut value: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        value["models"][0]["id"] = "../outside".into();
        assert!(ModelCatalog::from_json(&value.to_string()).is_err());

        let mut value: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        value["models"][0]["weights"]["url"] = "http://example.com/model".into();
        assert!(ModelCatalog::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut value: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        let duplicate = value["models"][0].clone();
        value["models"].as_array_mut().unwrap().push(duplicate);
        assert!(ModelCatalog::from_json(&value.to_string()).is_err());
    }
}
