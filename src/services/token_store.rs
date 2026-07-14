//! Append-only token cache: a flat little-endian i16 blob (row stride
//! `MAX_TOKEN_SEQ`) plus a JSON sidecar. Tracks the musical timeline (end_tick)
//! and the latest "setup" events used to prime each generation section. Mirrors
//! the Python `TokenStore`.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::tokenizer::codec::{Event, TokenRow, bos_row, tokens_to_event};
use crate::core::tokenizer::events::EventType;
use crate::core::tokenizer::vocab::{BOS_ID, EOS_ID, MAX_TOKEN_SEQ, PAD_ID};

const ROW_BYTES: usize = MAX_TOKEN_SEQ * 2;
const MAX_CONTROL_CHANGES: usize = 64;

#[derive(Default, Serialize, Deserialize)]
struct Timeline {
    coarse_time: i64,
    last_tick: i64,
    end_tick: i64,
}

#[derive(Serialize, Deserialize)]
struct Sidecar {
    width: usize,
    state: Vec<(String, Vec<i16>)>,
    timeline: Timeline,
}

/// Latest setup events (tempo/time-sig/key-sig/patch/CC), keyed like Python.
#[derive(Default)]
struct MusicalState {
    events: Vec<(String, TokenRow)>,
}

impl MusicalState {
    fn observe(&mut self, event: &Event, row: TokenRow) {
        let key = match event.kind {
            EventType::SetTempo => "set_tempo".to_string(),
            EventType::TimeSignature | EventType::KeySignature => {
                format!("{}:{}", event.kind.name(), event.params[2])
            }
            EventType::PatchChange => format!("patch_change:{}", event.params[3]),
            EventType::ControlChange => {
                format!("control_change:{}:{}", event.params[3], event.params[4])
            }
            EventType::Note => return,
        };
        // Store with time1/time2 zeroed (a normalized "current state" event).
        let mut normalized = event.clone();
        normalized.params[0] = 0;
        normalized.params[1] = 0;
        let Some(normalized_row) = crate::core::tokenizer::codec::event_to_tokens(&normalized)
        else {
            let _ = row;
            return;
        };
        self.events.retain(|(existing, _)| existing != &key);
        self.events.push((key, normalized_row));
        self.limit_control_changes();
    }

    fn limit_control_changes(&mut self) {
        let cc_count = self
            .events
            .iter()
            .filter(|(key, _)| key.starts_with("control_change:"))
            .count();
        if cc_count <= MAX_CONTROL_CHANGES {
            return;
        }
        let mut to_drop = cc_count - MAX_CONTROL_CHANGES;
        self.events.retain(|(key, _)| {
            if to_drop > 0 && key.starts_with("control_change:") {
                to_drop -= 1;
                false
            } else {
                true
            }
        });
    }

    fn rows(&self) -> Vec<TokenRow> {
        self.events.iter().map(|(_, row)| *row).collect()
    }

    fn to_sidecar(&self) -> Vec<(String, Vec<i16>)> {
        self.events
            .iter()
            .map(|(key, row)| (key.clone(), row.to_vec()))
            .collect()
    }

    fn load(&mut self, data: Vec<(String, Vec<i16>)>) {
        self.events = data
            .into_iter()
            .filter_map(|(key, row)| to_row(&row).map(|r| (key, r)))
            .collect();
    }
}

pub struct TokenStore {
    path: PathBuf,
    file: File,
    coarse_time: i64,
    last_tick: i64,
    end_tick: i64,
    state: MusicalState,
}

impl TokenStore {
    /// Create a fresh store, or clone `source`'s history if given (continuation).
    pub fn create(path: impl Into<PathBuf>, source: Option<&Path>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut store = Self {
            file: File::create(&path).with_context(|| format!("creating {}", path.display()))?,
            path,
            coarse_time: 0,
            last_tick: 0,
            end_tick: 0,
            state: MusicalState::default(),
        };
        if let Some(source) = source {
            store.copy_source(source)?;
        }
        store.file = OpenOptions::new().append(true).open(&store.path)?;
        Ok(store)
    }

    pub fn end_tick(&self) -> i64 {
        self.end_tick
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&mut self, row: &TokenRow) -> Result<()> {
        let mut bytes = [0u8; ROW_BYTES];
        for (i, &value) in row.iter().enumerate() {
            bytes[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        if let Some(event) = tokens_to_event(row) {
            self.state.observe(&event, *row);
            self.observe_timeline(&event);
        }
        Ok(())
    }

    pub fn extend(&mut self, rows: &[TokenRow]) -> Result<()> {
        for row in rows {
            self.append(row)?;
        }
        Ok(())
    }

    fn observe_timeline(&mut self, event: &Event) {
        self.coarse_time += i64::from(event.params[0]);
        let tick = (self.coarse_time * 16 + i64::from(event.params[1])) * 480 / 16;
        self.last_tick = self.last_tick.max(tick);
        self.end_tick = self.end_tick.max(self.last_tick);
        if event.kind == EventType::Note {
            let duration = i64::from(*event.params.last().unwrap());
            self.end_tick = self.end_tick.max(self.last_tick + duration * 480 / 16);
        }
    }

    pub fn count(&mut self) -> Result<usize> {
        self.file.flush()?;
        Ok(std::fs::metadata(&self.path)?.len() as usize / ROW_BYTES)
    }

    /// Read the last `count` rows from disk.
    pub fn tail(&mut self, count: usize) -> Result<Vec<TokenRow>> {
        self.file.flush()?;
        let total = self.count()?;
        let take = count.min(total);
        if take == 0 {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::End(-((take * ROW_BYTES) as i64)))?;
        let mut buffer = vec![0u8; take * ROW_BYTES];
        file.read_exact(&mut buffer)?;
        Ok(rows_from_bytes(&buffer))
    }

    /// Build a section prompt: bos + recent setup events + recent tail events.
    pub fn model_prompt(&mut self, context_size: usize) -> Result<Vec<TokenRow>> {
        let setup_capacity = (context_size / 3).clamp(1, 96);
        let setup_all = self.state.rows();
        let setup_start = setup_all.len().saturating_sub(setup_capacity);
        let setup = &setup_all[setup_start..];
        let tail_capacity = context_size.saturating_sub(setup.len() + 1).max(1);
        let tail: Vec<TokenRow> = self
            .tail(tail_capacity)?
            .into_iter()
            .filter(|row| {
                let head = row[0];
                head != BOS_ID as i16 && head != EOS_ID as i16
            })
            .collect();

        let mut prompt = Vec::with_capacity(1 + setup.len() + tail.len());
        prompt.push(bos_row(BOS_ID));
        prompt.extend_from_slice(setup);
        prompt.extend(tail);
        Ok(prompt)
    }

    /// Flush data and write the JSON sidecar.
    pub fn finish(mut self) -> Result<PathBuf> {
        self.file.flush()?;
        let sidecar = Sidecar {
            width: MAX_TOKEN_SEQ,
            state: self.state.to_sidecar(),
            timeline: Timeline {
                coarse_time: self.coarse_time,
                last_tick: self.last_tick,
                end_tick: self.end_tick,
            },
        };
        let sidecar_path = self.path.with_extension("json");
        std::fs::write(&sidecar_path, serde_json::to_vec(&sidecar)?)?;
        Ok(self.path)
    }

    fn copy_source(&mut self, source: &Path) -> Result<()> {
        std::fs::copy(source, &self.path)?;
        let sidecar_path = source.with_extension("json");
        if sidecar_path.is_file() {
            let sidecar: Sidecar = serde_json::from_slice(&std::fs::read(&sidecar_path)?)?;
            if sidecar.width != MAX_TOKEN_SEQ {
                bail!("cache created by an incompatible tokenizer version");
            }
            self.state.load(sidecar.state);
            self.coarse_time = sidecar.timeline.coarse_time;
            self.last_tick = sidecar.timeline.last_tick;
            self.end_tick = sidecar.timeline.end_tick;
        } else {
            self.scan_history()?;
        }
        Ok(())
    }

    fn scan_history(&mut self) -> Result<()> {
        let bytes = std::fs::read(&self.path)?;
        for row in rows_from_bytes(&bytes) {
            if let Some(event) = tokens_to_event(&row) {
                self.state.observe(&event, row);
                self.observe_timeline(&event);
            }
        }
        Ok(())
    }
}

/// Read every row from a token cache file.
pub fn read_rows(path: &Path) -> Result<Vec<TokenRow>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() % ROW_BYTES != 0 {
        bail!("token cache file is corrupted (not row-aligned)");
    }
    Ok(rows_from_bytes(&bytes))
}

fn to_row(values: &[i16]) -> Option<TokenRow> {
    values.try_into().ok()
}

fn rows_from_bytes(bytes: &[u8]) -> Vec<TokenRow> {
    bytes
        .chunks_exact(ROW_BYTES)
        .map(|chunk| {
            let mut row = [PAD_ID as i16; MAX_TOKEN_SEQ];
            for (i, slot) in row.iter_mut().enumerate() {
                *slot = i16::from_le_bytes([chunk[i * 2], chunk[i * 2 + 1]]);
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::codec::{Event, event_to_tokens};

    #[test]
    fn timeline_tracks_end_tick() {
        let dir = std::env::temp_dir().join(format!("ts_test_{}", std::process::id()));
        let path = dir.join("track.tokens");
        let mut store = TokenStore::create(&path, None).unwrap();

        // A note at time1=1, duration=16 sixteenths -> tick 480, end 480+480.
        let note = Event::new(EventType::Note, vec![1, 0, 0, 0, 60, 100, 16]);
        store.append(&event_to_tokens(&note).unwrap()).unwrap();
        assert_eq!(store.end_tick(), 480 + 480);

        store.finish().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
