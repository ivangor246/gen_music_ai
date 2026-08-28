//! Runtime model variants exposed to the shared application workflow.

use super::midi_gpt::MidiGptModel;
use super::midi_model::MidiModel;

pub enum GenerativeModel {
    Tv2o(MidiModel),
    MidiGpt(MidiGptModel),
}
