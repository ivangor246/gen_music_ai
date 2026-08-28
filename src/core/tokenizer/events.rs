//! Event and field definitions for tokenizer v2.
//!
//! This is the source of truth for the token layout. Enum declaration order
//! matches the checkpoint vocabulary, allowing `vocab.rs` to derive id ranges
//! from a variant's index.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Time1,
    Time2,
    Duration,
    Track,
    Channel,
    Pitch,
    Velocity,
    Patch,
    Controller,
    Value,
    Bpm,
    Nn,
    Dd,
    Sf,
    Mi,
}

impl Field {
    pub const fn name(self) -> &'static str {
        match self {
            Field::Time1 => "time1",
            Field::Time2 => "time2",
            Field::Duration => "duration",
            Field::Track => "track",
            Field::Channel => "channel",
            Field::Pitch => "pitch",
            Field::Velocity => "velocity",
            Field::Patch => "patch",
            Field::Controller => "controller",
            Field::Value => "value",
            Field::Bpm => "bpm",
            Field::Nn => "nn",
            Field::Dd => "dd",
            Field::Sf => "sf",
            Field::Mi => "mi",
        }
    }

    /// Number of distinct values in the checkpoint's token range.
    pub const fn size(self) -> u32 {
        match self {
            Field::Time1 => 128,
            Field::Time2 => 16,
            Field::Duration => 2048,
            Field::Track => 128,
            Field::Channel => 16,
            Field::Pitch => 128,
            Field::Velocity => 128,
            Field::Patch => 128,
            Field::Controller => 128,
            Field::Value => 128,
            Field::Bpm => 384,
            Field::Nn => 16,
            Field::Dd => 4,
            Field::Sf => 15,
            Field::Mi => 2,
        }
    }
}

/// Fields in checkpoint id-allocation order.
pub const FIELD_ORDER: [Field; 15] = [
    Field::Time1,
    Field::Time2,
    Field::Duration,
    Field::Track,
    Field::Channel,
    Field::Pitch,
    Field::Velocity,
    Field::Patch,
    Field::Controller,
    Field::Value,
    Field::Bpm,
    Field::Nn,
    Field::Dd,
    Field::Sf,
    Field::Mi,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Note,
    PatchChange,
    ControlChange,
    SetTempo,
    TimeSignature,
    KeySignature,
}

impl EventType {
    pub const fn name(self) -> &'static str {
        match self {
            EventType::Note => "note",
            EventType::PatchChange => "patch_change",
            EventType::ControlChange => "control_change",
            EventType::SetTempo => "set_tempo",
            EventType::TimeSignature => "time_signature",
            EventType::KeySignature => "key_signature",
        }
    }

    /// Parameter fields in slot order (slot 0 is the event-type id itself).
    pub const fn fields(self) -> &'static [Field] {
        match self {
            EventType::Note => &[
                Field::Time1,
                Field::Time2,
                Field::Track,
                Field::Channel,
                Field::Pitch,
                Field::Velocity,
                Field::Duration,
            ],
            EventType::PatchChange => &[
                Field::Time1,
                Field::Time2,
                Field::Track,
                Field::Channel,
                Field::Patch,
            ],
            EventType::ControlChange => &[
                Field::Time1,
                Field::Time2,
                Field::Track,
                Field::Channel,
                Field::Controller,
                Field::Value,
            ],
            EventType::SetTempo => &[Field::Time1, Field::Time2, Field::Track, Field::Bpm],
            EventType::TimeSignature => &[
                Field::Time1,
                Field::Time2,
                Field::Track,
                Field::Nn,
                Field::Dd,
            ],
            EventType::KeySignature => &[
                Field::Time1,
                Field::Time2,
                Field::Track,
                Field::Sf,
                Field::Mi,
            ],
        }
    }
}

/// Event types in checkpoint id-allocation order.
pub const EVENT_ORDER: [EventType; 6] = [
    EventType::Note,
    EventType::PatchChange,
    EventType::ControlChange,
    EventType::SetTempo,
    EventType::TimeSignature,
    EventType::KeySignature,
];
