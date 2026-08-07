//! Append-only token cache: a flat little-endian i16 blob plus a JSON sidecar.
//! Tracks the musical timeline and the latest setup events used to prime each
//! generation section.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::tokenizer::codec::{Event, TokenRow, bos_row, tokens_to_event};
use crate::core::tokenizer::events::EventType;
use crate::core::tokenizer::vocab::{BOS_ID, EOS_ID, MAX_TOKEN_SEQ, PAD_ID};

const ROW_BYTES: usize = MAX_TOKEN_SEQ * 2;
const MAX_CONTROL_CHANGES: usize = 64;
/// Upper bound on state rows prepended to a section prompt.
const MAX_SETUP_ROWS: usize = 96;

pub fn clear_cache(directory: &Path) -> Result<usize> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", directory.display()));
        }
    };

    let mut removed = 0;
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading an entry in {}", directory.display()))?
            .path();
        if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("tokens" | "json")
        ) {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

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

/// Latest setup events keyed by their musical role and channel.
#[derive(Default)]
struct MusicalState {
    events: Vec<(String, TokenRow)>,
}

/// Key of the state slot an event updates, or `None` for events that carry no
/// lasting state.
fn setup_key(event: &Event) -> Option<String> {
    Some(match event.kind {
        EventType::SetTempo => "set_tempo".to_string(),
        EventType::TimeSignature | EventType::KeySignature => {
            format!("{}:{}", event.kind.name(), event.params[2])
        }
        EventType::PatchChange => format!("patch_change:{}", event.params[3]),
        EventType::ControlChange => {
            format!("control_change:{}:{}", event.params[3], event.params[4])
        }
        EventType::Note => return None,
    })
}

impl MusicalState {
    fn observe(&mut self, event: &Event, row: TokenRow) {
        let Some(key) = setup_key(event) else {
            return;
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

    /// Number of state slots currently tracked.
    fn len(&self) -> usize {
        self.events.len()
    }

    /// Most recent state rows whose slot is not already `covered`, newest last.
    fn rows_beyond(&self, covered: &HashSet<String>, limit: usize) -> Vec<TokenRow> {
        let rows: Vec<TokenRow> = self
            .events
            .iter()
            .filter(|(key, _)| !covered.contains(key))
            .map(|(_, row)| *row)
            .collect();
        let start = rows.len().saturating_sub(limit);
        rows[start..].to_vec()
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
    /// Buffered: generation appends one 16-byte row per event per track.
    file: BufWriter<File>,
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
            file: BufWriter::new(
                File::create(&path).with_context(|| format!("creating {}", path.display()))?,
            ),
            path,
            coarse_time: 0,
            last_tick: 0,
            end_tick: 0,
            state: MusicalState::default(),
        };
        if let Some(source) = source {
            store.copy_source(source)?;
        }
        store.file = BufWriter::new(OpenOptions::new().append(true).open(&store.path)?);
        Ok(store)
    }

    /// Tick of the latest event onset. This is how far the composition has been
    /// written, which is what drives the stop condition -- unlike `end_tick`,
    /// which also covers how long the last note keeps ringing.
    pub fn last_tick(&self) -> i64 {
        self.last_tick
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

    /// `last_tick` follows event onsets; `end_tick` additionally covers the tail
    /// of the longest note, so it is the right length for export but the wrong
    /// measure of progress.
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

    /// Build a section prompt: bos + the setup state the tail does not already
    /// carry + the recent tail events.
    pub fn model_prompt(&mut self, context_size: usize) -> Result<Vec<TokenRow>> {
        let capacity = context_size.max(1);
        let setup_limit = (context_size / 3).clamp(1, MAX_SETUP_ROWS);
        // Reserve room for the state block before picking the tail. The tail has
        // to be final before its setup keys are read: trimming it afterwards
        // could drop a setup event that had already been counted as covered,
        // losing that state from the prompt entirely.
        let reserve = self.state.len().min(setup_limit);
        let tail_budget = capacity.saturating_sub(1 + reserve).max(1);

        let tail: Vec<TokenRow> = self
            .tail(tail_budget)?
            .into_iter()
            .filter(|row| {
                let head = row[0];
                head != BOS_ID as i16 && head != EOS_ID as i16
            })
            .collect();

        // Setup events inside the tail window are already in the prompt at their
        // real position; prepending them again would show the model two of each
        // back to back at delta zero, which the checkpoint never saw in training.
        let covered: HashSet<String> = tail
            .iter()
            .filter_map(|row| tokens_to_event(row).as_ref().and_then(setup_key))
            .collect();
        let setup = self.state.rows_beyond(&covered, setup_limit);

        let mut prompt = Vec::with_capacity(1 + setup.len() + tail.len());
        prompt.push(bos_row(BOS_ID));
        prompt.extend(setup);
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
    use crate::core::tokenizer::vocab::event_type_id;

    fn store_at(name: &str) -> (PathBuf, TokenStore) {
        let dir = std::env::temp_dir().join(format!("ts_{name}_{}", std::process::id()));
        let path = dir.join("track.tokens");
        let store = TokenStore::create(&path, None).unwrap();
        (dir, store)
    }

    fn append(store: &mut TokenStore, event: &Event) {
        store.append(&event_to_tokens(event).unwrap()).unwrap();
    }

    #[test]
    fn cache_clear_preserves_unrelated_files() {
        let dir = std::env::temp_dir().join(format!("cache_clear_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("track.tokens"), []).unwrap();
        std::fs::write(dir.join("track.json"), []).unwrap();
        std::fs::write(dir.join("export.mid"), []).unwrap();

        assert_eq!(clear_cache(&dir).unwrap(), 2);
        assert!(!dir.join("track.tokens").exists());
        assert!(!dir.join("track.json").exists());
        assert!(dir.join("export.mid").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn onset_and_ring_out_are_tracked_separately() {
        let (dir, mut store) = store_at("ticks");

        // A note at time1=1, duration=16 sixteenths -> onset 480, ring-out 960.
        append(
            &mut store,
            &Event::new(EventType::Note, vec![1, 0, 0, 0, 60, 100, 16]),
        );
        assert_eq!(store.last_tick(), 480);
        assert_eq!(store.end_tick, 480 + 480);

        // A very long note moves the ring-out far past the onset; progress must
        // keep following the onset, or one such note ends the track early.
        append(
            &mut store,
            &Event::new(EventType::Note, vec![0, 0, 0, 0, 48, 100, 2047]),
        );
        assert_eq!(store.last_tick(), 480);
        assert_eq!(store.end_tick, 480 + 2047 * 30);

        store.finish().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn section_prompt_keeps_one_copy_of_each_setup_event() {
        let (dir, mut store) = store_at("prompt_dedupe");
        store.append(&bos_row(BOS_ID)).unwrap();
        append(
            &mut store,
            &Event::new(EventType::TimeSignature, vec![0, 0, 0, 3, 1]),
        );
        append(
            &mut store,
            &Event::new(EventType::SetTempo, vec![0, 0, 0, 120]),
        );
        append(
            &mut store,
            &Event::new(EventType::PatchChange, vec![0, 0, 1, 0, 40]),
        );

        // The tail covers the whole file, so nothing needs re-injecting: bos
        // plus one copy of each event, not two.
        let prompt = store.model_prompt(64).unwrap();
        assert_eq!(prompt.len(), 4);
        assert_eq!(prompt[0], bos_row(BOS_ID));
        for kind in [
            EventType::TimeSignature,
            EventType::SetTempo,
            EventType::PatchChange,
        ] {
            let id = event_type_id(kind) as i16;
            assert_eq!(
                prompt.iter().filter(|row| row[0] == id).count(),
                1,
                "{kind:?} should appear once"
            );
        }

        store.finish().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Trimming the tail after reading its setup keys used to lose state: the
    /// trimmed rows still counted as "covered", so they were neither kept in the
    /// tail nor re-added at the head.
    #[test]
    fn section_prompt_keeps_setup_trimmed_off_the_tail() {
        let (dir, mut store) = store_at("prompt_trim");
        store.append(&bos_row(BOS_ID)).unwrap();
        append(
            &mut store,
            &Event::new(EventType::TimeSignature, vec![0, 0, 0, 3, 1]),
        );
        append(
            &mut store,
            &Event::new(EventType::SetTempo, vec![0, 0, 0, 120]),
        );
        append(
            &mut store,
            &Event::new(EventType::PatchChange, vec![0, 0, 1, 0, 40]),
        );
        for _ in 0..6 {
            append(
                &mut store,
                &Event::new(EventType::Note, vec![1, 0, 1, 0, 60, 100, 8]),
            );
        }

        let prompt = store.model_prompt(9).unwrap();
        assert!(prompt.len() <= 9, "prompt must fit the context window");
        for kind in [
            EventType::TimeSignature,
            EventType::SetTempo,
            EventType::PatchChange,
        ] {
            let id = event_type_id(kind) as i16;
            assert_eq!(
                prompt.iter().filter(|row| row[0] == id).count(),
                1,
                "{kind:?} must survive exactly once"
            );
        }

        store.finish().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn section_prompt_restores_setup_that_scrolled_out() {
        let (dir, mut store) = store_at("prompt_restore");
        store.append(&bos_row(BOS_ID)).unwrap();
        append(
            &mut store,
            &Event::new(EventType::PatchChange, vec![0, 0, 1, 0, 40]),
        );
        for _ in 0..10 {
            append(
                &mut store,
                &Event::new(EventType::Note, vec![1, 0, 1, 0, 60, 100, 8]),
            );
        }

        // The patch change is far outside a six-row window, so it has to come
        // back at the head or the section loses the instrument.
        let prompt = store.model_prompt(6).unwrap();
        assert_eq!(prompt.len(), 6);
        assert_eq!(prompt[1][0], event_type_id(EventType::PatchChange) as i16);

        store.finish().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
