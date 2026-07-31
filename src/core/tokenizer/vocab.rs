//! Token id layout for tokenizer v2, derived from the event/field definitions.
//!
//! Allocation order: pad, bos, eos, then one id per event type in `EVENT_ORDER`,
//! followed by contiguous ranges per field in `FIELD_ORDER`. All ids are
//! computed from those tables so they stay in sync with `events.rs`.

use super::events::{EVENT_ORDER, EventType, FIELD_ORDER, Field};

pub const PAD_ID: u32 = 0;
pub const BOS_ID: u32 = 1;
pub const EOS_ID: u32 = 2;
pub const MAX_TOKEN_SEQ: usize = 8;

const SPECIAL_COUNT: u32 = 3;
const FIRST_EVENT_ID: u32 = SPECIAL_COUNT;
const FIRST_PARAM_ID: u32 = FIRST_EVENT_ID + EVENT_ORDER.len() as u32;

const fn compute_vocab_size() -> u32 {
    let mut total = FIRST_PARAM_ID;
    let mut i = 0;
    while i < FIELD_ORDER.len() {
        total += FIELD_ORDER[i].size();
        i += 1;
    }
    total
}

pub const VOCAB_SIZE: u32 = compute_vocab_size();

/// Token id for an event-type slot. Variant index matches the allocation order.
pub const fn event_type_id(event: EventType) -> u32 {
    FIRST_EVENT_ID + event as u32
}

/// Inclusive lower bound of a field's id range.
pub const fn field_base(field: Field) -> u32 {
    let target = field as usize;
    let mut base = FIRST_PARAM_ID;
    let mut i = 0;
    while i < target {
        base += FIELD_ORDER[i].size();
        i += 1;
    }
    base
}

/// Token id for a concrete field value (`value` must be `< field.size()`).
pub const fn field_token(field: Field, value: u32) -> u32 {
    field_base(field) + value
}

/// Recover an event type from a slot-0 token id, if it is an event id.
pub fn event_type_from_id(id: u32) -> Option<EventType> {
    let index = id.checked_sub(FIRST_EVENT_ID)? as usize;
    EVENT_ORDER.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_size_matches_checkpoint() {
        assert_eq!(VOCAB_SIZE, 3406);
    }

    #[test]
    fn event_ids_match_checkpoint_layout() {
        assert_eq!(event_type_id(EventType::Note), 3);
        assert_eq!(event_type_id(EventType::PatchChange), 4);
        assert_eq!(event_type_id(EventType::ControlChange), 5);
        assert_eq!(event_type_id(EventType::SetTempo), 6);
        assert_eq!(event_type_id(EventType::TimeSignature), 7);
        assert_eq!(event_type_id(EventType::KeySignature), 8);
    }

    #[test]
    fn field_ranges_match_checkpoint_layout() {
        // Representative bases/bounds from the documented v2 layout.
        assert_eq!(field_base(Field::Time1), 9);
        assert_eq!(field_base(Field::Time2), 137);
        assert_eq!(field_base(Field::Duration), 153);
        assert_eq!(field_base(Field::Track), 2201);
        assert_eq!(field_base(Field::Channel), 2329);
        assert_eq!(field_base(Field::Pitch), 2345);
        assert_eq!(field_base(Field::Bpm), 2985);
        assert_eq!(field_base(Field::Mi), 3404);
        // Last id of the last field is VOCAB_SIZE - 1.
        assert_eq!(field_token(Field::Mi, Field::Mi.size() - 1), VOCAB_SIZE - 1);
    }

    #[test]
    fn event_type_roundtrip() {
        for event in EVENT_ORDER {
            assert_eq!(event_type_from_id(event_type_id(event)), Some(event));
        }
        assert_eq!(event_type_from_id(PAD_ID), None);
        assert_eq!(event_type_from_id(EOS_ID), None);
    }
}
