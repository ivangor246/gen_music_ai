//! Row <-> event codec for tokenizer v2, mirroring `event2tokens` /
//! `tokens2event`. A "row" is `MAX_TOKEN_SEQ` token ids (stored as i16 in the
//! cache); an `Event` is an event type plus its decoded field *values* in slot
//! order.

use super::events::EventType;
use super::vocab::{
    MAX_TOKEN_SEQ, PAD_ID, event_type_from_id, event_type_id, field_base, field_token,
};

pub type TokenRow = [i16; MAX_TOKEN_SEQ];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub kind: EventType,
    /// Field values in slot order (e.g. note: time1, time2, track, channel, pitch, velocity, duration).
    pub params: Vec<u16>,
}

impl Event {
    pub fn new(kind: EventType, params: Vec<u16>) -> Self {
        Self { kind, params }
    }
}

/// A padded row holding only bos in slot 0.
pub fn bos_row(bos_id: u32) -> TokenRow {
    let mut row = [PAD_ID as i16; MAX_TOKEN_SEQ];
    row[0] = bos_id as i16;
    row
}

/// Encode an event into a padded token row, or None if any value is out of range.
pub fn event_to_tokens(event: &Event) -> Option<TokenRow> {
    let fields = event.kind.fields();
    if event.params.len() != fields.len() {
        return None;
    }
    let mut row = [PAD_ID as i16; MAX_TOKEN_SEQ];
    row[0] = event_type_id(event.kind) as i16;
    for (slot, (&field, &value)) in fields.iter().zip(&event.params).enumerate() {
        if u32::from(value) >= field.size() {
            return None;
        }
        row[slot + 1] = field_token(field, u32::from(value)) as i16;
    }
    Some(row)
}

/// Decode a token row into an event, or None for bos/eos/pad or malformed rows.
pub fn tokens_to_event(row: &[i16]) -> Option<Event> {
    let id = u32::try_from(*row.first()?).ok()?;
    let kind = event_type_from_id(id)?;
    let fields = kind.fields();
    if row.len() <= fields.len() {
        return None;
    }
    let mut params = Vec::with_capacity(fields.len());
    for (slot, &field) in fields.iter().enumerate() {
        let value = i64::from(row[slot + 1]) - i64::from(field_base(field));
        if value < 0 || value as u32 >= field.size() {
            return None;
        }
        params.push(value as u16);
    }
    Some(Event { kind, params })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::vocab::BOS_ID;

    #[test]
    fn note_roundtrip() {
        let event = Event::new(EventType::Note, vec![5, 3, 1, 9, 60, 100, 240]);
        let row = event_to_tokens(&event).unwrap();
        assert_eq!(tokens_to_event(&row), Some(event));
    }

    #[test]
    fn bos_and_pad_decode_to_none() {
        assert_eq!(tokens_to_event(&bos_row(BOS_ID)), None);
        assert_eq!(tokens_to_event(&[0i16; MAX_TOKEN_SEQ]), None);
    }

    #[test]
    fn out_of_range_value_rejected() {
        // pitch max is 128, so 200 is invalid.
        let event = Event::new(EventType::Note, vec![0, 0, 0, 0, 200, 100, 10]);
        assert_eq!(event_to_tokens(&event), None);
    }
}
