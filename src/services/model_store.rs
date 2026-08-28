//! Verified, resumable storage for independently downloaded model checkpoints.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::model::config::ModelConfig;
use crate::paths::models_dir;

use super::atomic::atomic_write;
use super::model_catalog::{ModelArtifact, ModelDescriptor};

const CONFIG_FILE: &str = "config.json";
const WEIGHTS_FILE: &str = "model.safetensors";
const INSTALL_FILE: &str = "install.json";
const BUFFER_SIZE: usize = 128 * 1024;
const CANCELLED: &str = "model download cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalModelState {
    NotInstalled,
    Partial,
    Installed,
}

pub struct ModelBundle {
    pub config: ModelConfig,
    pub weights: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct InstallRecord {
    id: String,
    format: String,
    config_sha256: String,
    weights_sha256: String,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::new(models_dir())
    }
}

impl ModelStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn local_state(&self, model: &ModelDescriptor) -> LocalModelState {
        let directory = self.model_dir(model);
        if !directory.exists() {
            return LocalModelState::NotInstalled;
        }
        let record = std::fs::read_to_string(directory.join(INSTALL_FILE))
            .ok()
            .and_then(|text| serde_json::from_str::<InstallRecord>(&text).ok());
        if record
            .as_ref()
            .is_some_and(|record| record_matches(record, model))
            && file_has_size(&directory.join(CONFIG_FILE), model.config.size)
            && file_has_size(&directory.join(WEIGHTS_FILE), model.weights.size)
        {
            LocalModelState::Installed
        } else {
            LocalModelState::Partial
        }
    }

    pub fn download(
        &self,
        model: &ModelDescriptor,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<()> {
        let directory = self.model_dir(model);
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        std::fs::remove_file(directory.join(INSTALL_FILE)).ok();

        let client = download_client()?;
        let total = model.download_size();
        download_artifact(
            &client,
            &model.config,
            &directory.join(CONFIG_FILE),
            0,
            total,
            cancel,
            &mut progress,
        )?;
        download_artifact(
            &client,
            &model.weights,
            &directory.join(WEIGHTS_FILE),
            model.config.size,
            total,
            cancel,
            &mut progress,
        )?;
        if cancel.load(Ordering::Relaxed) {
            bail!(CANCELLED);
        }

        let record = InstallRecord {
            id: model.id.clone(),
            format: model.format.clone(),
            config_sha256: model.config.sha256.clone(),
            weights_sha256: model.weights.sha256.clone(),
        };
        atomic_write(&directory.join(INSTALL_FILE), |file| {
            serde_json::to_writer_pretty(file, &record).context("writing model install metadata")
        })?;
        Ok(())
    }

    pub fn load_bundle(&self, model: &ModelDescriptor) -> Result<ModelBundle> {
        if self.local_state(model) != LocalModelState::Installed {
            bail!("model `{}` is not completely installed", model.name);
        }
        let directory = self.model_dir(model);
        let config_bytes = read_verified(&directory.join(CONFIG_FILE), &model.config)?;
        let config_json = std::str::from_utf8(&config_bytes).context("config.json is not UTF-8")?;
        let config = ModelConfig::from_compatible_json(config_json)?;
        let weights = read_verified(&directory.join(WEIGHTS_FILE), &model.weights)?;
        Ok(ModelBundle { config, weights })
    }

    pub fn remove(&self, model: &ModelDescriptor) -> Result<()> {
        let directory = self.model_dir(model);
        if directory.exists() {
            std::fs::remove_dir_all(&directory)
                .with_context(|| format!("removing {}", directory.display()))?;
        }
        Ok(())
    }

    fn model_dir(&self, model: &ModelDescriptor) -> PathBuf {
        self.root.join(&model.id)
    }
}

pub fn was_cancelled(error: &anyhow::Error) -> bool {
    error.to_string() == CANCELLED
}

fn record_matches(record: &InstallRecord, model: &ModelDescriptor) -> bool {
    record.id == model.id
        && record.format == model.format
        && record.config_sha256 == model.config.sha256
        && record.weights_sha256 == model.weights.sha256
}

fn file_has_size(path: &Path, expected: u64) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.len() == expected)
}

fn download_client() -> Result<Client> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(6 * 60 * 60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("creating the model download client")
}

#[allow(clippy::too_many_arguments)]
fn download_artifact(
    client: &Client,
    artifact: &ModelArtifact,
    target: &Path,
    base_progress: u64,
    total_progress: u64,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
) -> Result<()> {
    if artifact_matches(target, artifact, Some(cancel))? {
        progress(base_progress + artifact.size, total_progress);
        return Ok(());
    }
    if target.exists() {
        std::fs::remove_file(target)
            .with_context(|| format!("removing invalid {}", target.display()))?;
    }

    let partial = target.with_extension(format!(
        "{}.part",
        target
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("file")
    ));
    let mut offset = partial.metadata().map_or(0, |metadata| metadata.len());
    if offset > artifact.size {
        std::fs::remove_file(&partial)
            .with_context(|| format!("removing oversized {}", partial.display()))?;
        offset = 0;
    } else if offset == artifact.size {
        if artifact_matches(&partial, artifact, Some(cancel))? {
            finalize_partial(&partial, target)?;
            progress(base_progress + artifact.size, total_progress);
            return Ok(());
        }
        std::fs::remove_file(&partial)
            .with_context(|| format!("removing invalid {}", partial.display()))?;
        offset = 0;
    }
    if cancel.load(Ordering::Relaxed) {
        bail!(CANCELLED);
    }

    let mut request = client.get(&artifact.url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .with_context(|| format!("downloading {}", artifact.url))?;
    if !response.status().is_success() {
        bail!("download returned HTTP {}", response.status());
    }

    let append = offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if append {
        validate_content_range(&response, offset, artifact.size)?;
    } else {
        offset = 0;
    }
    validate_content_length(&response, artifact.size - offset)?;

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&partial)
        .with_context(|| format!("opening {}", partial.display()))?;
    copy_limited(
        &mut response,
        &mut file,
        offset,
        artifact.size,
        cancel,
        |written| progress(base_progress + written, total_progress),
    )?;
    file.sync_all().ok();
    drop(file);

    if !artifact_matches(&partial, artifact, Some(cancel))? {
        std::fs::remove_file(&partial).ok();
        bail!("SHA-256 mismatch for downloaded model artifact");
    }
    finalize_partial(&partial, target)?;
    Ok(())
}

fn validate_content_length(response: &Response, expected: u64) -> Result<()> {
    let Some(value) = response.headers().get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let actual = value
        .to_str()
        .context("invalid Content-Length header")?
        .parse::<u64>()
        .context("invalid Content-Length value")?;
    if actual != expected {
        bail!("download length {actual} does not match expected length {expected}");
    }
    Ok(())
}

fn validate_content_range(response: &Response, offset: u64, expected: u64) -> Result<()> {
    let value = response
        .headers()
        .get(CONTENT_RANGE)
        .context("resumed download omitted Content-Range")?
        .to_str()
        .context("invalid Content-Range header")?;
    validate_content_range_value(value, offset, expected)
}

fn validate_content_range_value(value: &str, offset: u64, expected: u64) -> Result<()> {
    let value = value
        .strip_prefix("bytes ")
        .context("unsupported Content-Range unit")?;
    let (range, total) = value
        .split_once('/')
        .context("invalid Content-Range value")?;
    let (start, end) = range
        .split_once('-')
        .context("invalid Content-Range bounds")?;
    let start = start
        .parse::<u64>()
        .context("invalid Content-Range start")?;
    let end = end.parse::<u64>().context("invalid Content-Range end")?;
    let total = total
        .parse::<u64>()
        .context("invalid Content-Range total")?;
    if start != offset || total != expected || end < start || end >= total {
        bail!("Content-Range does not match the requested artifact");
    }
    Ok(())
}

fn copy_limited(
    reader: &mut impl Read,
    writer: &mut impl Write,
    initial: u64,
    expected: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let mut written = initial;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    while written < expected {
        if cancel.load(Ordering::Relaxed) {
            bail!(CANCELLED);
        }
        let read = reader.read(&mut buffer).context("reading model download")?;
        if read == 0 {
            break;
        }
        let next = written
            .checked_add(read as u64)
            .context("model download size overflow")?;
        if next > expected {
            bail!("download exceeded the expected artifact size");
        }
        writer
            .write_all(&buffer[..read])
            .context("writing model download")?;
        written = next;
        progress(written);
    }
    if written != expected {
        bail!("download ended at {written} bytes; expected {expected}");
    }
    let mut extra = [0_u8; 1];
    if reader
        .read(&mut extra)
        .context("checking model download size")?
        != 0
    {
        bail!("download exceeded the expected artifact size");
    }
    Ok(())
}

fn finalize_partial(partial: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        std::fs::remove_file(target).with_context(|| format!("removing {}", target.display()))?;
    }
    std::fs::rename(partial, target).with_context(|| format!("installing {}", target.display()))
}

fn read_verified(path: &Path, artifact: &ModelArtifact) -> Result<Vec<u8>> {
    let metadata = path
        .metadata()
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    if metadata.len() != artifact.size {
        bail!(
            "{} has size {}, expected {}",
            path.display(),
            metadata.len(),
            artifact.size
        );
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if sha256_bytes(&bytes) != artifact.sha256 {
        bail!("SHA-256 mismatch for {}", path.display());
    }
    Ok(bytes)
}

fn artifact_matches(
    path: &Path,
    artifact: &ModelArtifact,
    cancel: Option<&AtomicBool>,
) -> Result<bool> {
    let Ok(metadata) = path.metadata() else {
        return Ok(false);
    };
    if metadata.len() != artifact.size {
        return Ok(false);
    }
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(sha256_reader(file, cancel)? == artifact.sha256)
}

fn sha256_reader(mut reader: impl Read, cancel: Option<&AtomicBool>) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    loop {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            bail!(CANCELLED);
        }
        let read = reader.read(&mut buffer).context("hashing model artifact")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(&hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::services::model_catalog::{ModelCatalog, SUPPORTED_FORMAT};

    fn temp_store() -> (PathBuf, ModelStore) {
        let root = std::env::temp_dir().join(format!(
            "model_store_test_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        (root.clone(), ModelStore::new(root))
    }

    fn fixture_model(config: &[u8], weights: &[u8]) -> ModelDescriptor {
        ModelDescriptor {
            id: "fixture-model".to_string(),
            name: "Fixture".to_string(),
            description: "Fixture".to_string(),
            format: SUPPORTED_FORMAT.to_string(),
            source_url: "https://example.com/model".to_string(),
            license: "Apache-2.0".to_string(),
            config: ModelArtifact {
                url: "https://example.com/config".to_string(),
                size: config.len() as u64,
                sha256: sha256_bytes(config),
            },
            weights: ModelArtifact {
                url: "https://example.com/weights".to_string(),
                size: weights.len() as u64,
                sha256: sha256_bytes(weights),
            },
        }
    }

    #[test]
    fn built_in_artifact_hashes_match_local_fixtures() {
        let model = ModelCatalog::load().unwrap().default_model().clone();
        assert_eq!(
            sha256_bytes(crate::services::model_catalog::DEFAULT_CONFIG_JSON.as_bytes()),
            model.config.sha256
        );
    }

    #[test]
    fn install_state_requires_matching_record_and_files() {
        let config = b"config";
        let weights = b"weights";
        let model = fixture_model(config, weights);
        let (root, store) = temp_store();
        let directory = store.model_dir(&model);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(CONFIG_FILE), config).unwrap();
        std::fs::write(directory.join(WEIGHTS_FILE), weights).unwrap();
        assert_eq!(store.local_state(&model), LocalModelState::Partial);

        let record = InstallRecord {
            id: model.id.clone(),
            format: model.format.clone(),
            config_sha256: model.config.sha256.clone(),
            weights_sha256: model.weights.sha256.clone(),
        };
        std::fs::write(
            directory.join(INSTALL_FILE),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(store.local_state(&model), LocalModelState::Installed);

        store.remove(&model).unwrap();
        assert_eq!(store.local_state(&model), LocalModelState::NotInstalled);
        assert!(!directory.exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn bounded_copy_rejects_short_and_oversized_responses() {
        let cancel = AtomicBool::new(false);
        let mut output = Vec::new();
        assert!(
            copy_limited(
                &mut Cursor::new(b"short"),
                &mut output,
                0,
                8,
                &cancel,
                |_| {}
            )
            .is_err()
        );

        let mut output = Vec::new();
        assert!(
            copy_limited(
                &mut Cursor::new(b"too large"),
                &mut output,
                0,
                4,
                &cancel,
                |_| {}
            )
            .is_err()
        );
    }

    #[test]
    fn bounded_copy_honours_cancellation() {
        let cancel = AtomicBool::new(true);
        let mut output = Vec::new();
        let error = copy_limited(
            &mut Cursor::new(b"data"),
            &mut output,
            0,
            4,
            &cancel,
            |_| {},
        )
        .unwrap_err();
        assert!(was_cancelled(&error));
        assert!(output.is_empty());
    }

    #[test]
    fn content_range_must_match_requested_offset_and_size() {
        assert!(validate_content_range_value("bytes 4-9/10", 4, 10).is_ok());
        assert!(validate_content_range_value("bytes 3-9/10", 4, 10).is_err());
        assert!(validate_content_range_value("bytes 4-9/11", 4, 10).is_err());
    }
}
