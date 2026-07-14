//! Save a generated track's token cache as a Standard MIDI File.

use std::path::Path;

use anyhow::Result;

use crate::core::midi::score::ActionStream;
use crate::core::midi::smf::write_midi;
use crate::services::atomic::atomic_write;
use crate::services::token_store::read_rows;

pub fn save_midi(token_path: &Path, out_path: &Path, target_tick: Option<i64>) -> Result<()> {
    let rows = read_rows(token_path)?;
    atomic_write(out_path, |file| {
        let stream = ActionStream::new(rows.into_iter());
        write_midi(stream, target_tick, file)?;
        Ok(())
    })
}
