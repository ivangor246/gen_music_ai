//! Per-slot legal-id masks for constrained decoding.
//!
//! A slot's legal set depends only on the run's flags, never on what has been
//! generated, so every set is built once here and handed out as a slice. The
//! decode loop asks for them `batch * MAX_TOKEN_SEQ` times per event.

use crate::core::tokenizer::events::{EVENT_ORDER, EventType, Field};
use crate::core::tokenizer::vocab::{
    EOS_ID, MAX_TOKEN_SEQ, PAD_ID, event_type_id, field_base, field_token,
};

pub struct DecodeFlags {
    event_ids: Vec<u32>,
    event_ids_with_eos: Vec<u32>,
    /// Legal ids per (event type, slot), indexed by `param_index`.
    param_ids: Vec<Vec<u32>>,
}

impl DecodeFlags {
    pub fn new(
        disable_patch_change: bool,
        disable_control_change: bool,
        disable_tempo_change: bool,
        disabled_channels: &[u16],
    ) -> Self {
        let disabled_channel_ids: Vec<u32> = disabled_channels
            .iter()
            .map(|&channel| field_token(Field::Channel, u32::from(channel)))
            .collect();

        let event_ids: Vec<u32> = EVENT_ORDER
            .iter()
            .filter(|&&event| !match event {
                EventType::PatchChange => disable_patch_change,
                EventType::ControlChange => disable_control_change,
                EventType::SetTempo => disable_tempo_change,
                _ => false,
            })
            .map(|&event| event_type_id(event))
            .collect();
        let mut event_ids_with_eos = event_ids.clone();
        event_ids_with_eos.push(EOS_ID);

        let mut param_ids = vec![Vec::new(); EVENT_ORDER.len() * MAX_TOKEN_SEQ];
        for &kind in &EVENT_ORDER {
            for slot in 1..MAX_TOKEN_SEQ {
                param_ids[param_index(kind, slot)] =
                    param_ids_for(kind, slot, &disabled_channel_ids);
            }
        }

        Self {
            event_ids,
            event_ids_with_eos,
            param_ids,
        }
    }

    /// Legal ids for slot 0: enabled event types, plus eos when `allow_eos` is
    /// set. Callers gate `allow_eos` on generation progress so the model cannot
    /// end a track far short of its requested length.
    pub fn event_ids(&self, allow_eos: bool) -> &[u32] {
        if allow_eos {
            &self.event_ids_with_eos
        } else {
            &self.event_ids
        }
    }

    /// Legal ids for slot `slot` (>= 1) of an event of type `kind`.
    pub fn param_ids(&self, kind: EventType, slot: usize) -> &[u32] {
        &self.param_ids[param_index(kind, slot)]
    }
}

fn param_index(kind: EventType, slot: usize) -> usize {
    kind as usize * MAX_TOKEN_SEQ + slot
}

/// Beyond the event's parameter count only pad is legal; the channel field
/// drops disabled channels.
fn param_ids_for(kind: EventType, slot: usize, disabled_channel_ids: &[u32]) -> Vec<u32> {
    let fields = kind.fields();
    if slot > fields.len() {
        return vec![PAD_ID];
    }
    let field = fields[slot - 1];
    let base = field_base(field);
    let ids = (0..field.size()).map(|value| base + value);
    if field == Field::Channel {
        ids.filter(|id| !disabled_channel_ids.contains(id))
            .collect()
    } else {
        ids.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_respect_disables() {
        let flags = DecodeFlags::new(true, true, true, &[]);
        let ids = flags.event_ids(true);
        // note, key_signature, time_signature remain; patch/cc/tempo removed; eos present.
        assert!(ids.contains(&event_type_id(EventType::Note)));
        assert!(!ids.contains(&event_type_id(EventType::PatchChange)));
        assert!(!ids.contains(&event_type_id(EventType::ControlChange)));
        assert!(!ids.contains(&event_type_id(EventType::SetTempo)));
        assert!(ids.contains(&EOS_ID));
    }

    #[test]
    fn eos_dropped_when_not_allowed() {
        let flags = DecodeFlags::new(false, false, false, &[]);
        assert!(!flags.event_ids(false).contains(&EOS_ID));
    }

    #[test]
    fn channel_slot_filters_disabled() {
        let flags = DecodeFlags::new(false, false, true, &[3]);
        // note slot 4 is the channel field.
        let ids = flags.param_ids(EventType::Note, 4);
        assert!(!ids.contains(&field_token(Field::Channel, 3)));
        assert!(ids.contains(&field_token(Field::Channel, 0)));
    }

    #[test]
    fn beyond_params_is_pad() {
        let flags = DecodeFlags::new(false, false, true, &[]);
        // set_tempo has 4 params; slot 5 must be pad.
        assert_eq!(flags.param_ids(EventType::SetTempo, 5), [PAD_ID]);
    }

    #[test]
    fn every_event_and_slot_has_a_legal_set() {
        let flags = DecodeFlags::new(false, false, false, &[]);
        for kind in EVENT_ORDER {
            for slot in 1..MAX_TOKEN_SEQ {
                assert!(
                    !flags.param_ids(kind, slot).is_empty(),
                    "{kind:?} slot {slot} has no legal id"
                );
            }
        }
    }
}
