//! Per-slot legal-id masks for constrained decoding.

use crate::core::tokenizer::events::{EVENT_ORDER, EventType, Field};
use crate::core::tokenizer::vocab::{EOS_ID, PAD_ID, event_type_id, field_base, field_token};

pub struct DecodeFlags {
    pub disable_patch_change: bool,
    pub disable_control_change: bool,
    pub disable_tempo_change: bool,
    disabled_channel_ids: Vec<u32>,
}

impl DecodeFlags {
    pub fn new(
        disable_patch_change: bool,
        disable_control_change: bool,
        disable_tempo_change: bool,
        disabled_channels: &[u16],
    ) -> Self {
        let disabled_channel_ids = disabled_channels
            .iter()
            .map(|&channel| field_token(Field::Channel, u32::from(channel)))
            .collect();
        Self {
            disable_patch_change,
            disable_control_change,
            disable_tempo_change,
            disabled_channel_ids,
        }
    }

    fn is_event_disabled(&self, event: EventType) -> bool {
        match event {
            EventType::PatchChange => self.disable_patch_change,
            EventType::ControlChange => self.disable_control_change,
            EventType::SetTempo => self.disable_tempo_change,
            _ => false,
        }
    }
}

/// Allowed ids for slot 0: enabled event types plus eos.
pub fn allowed_event_ids(flags: &DecodeFlags) -> Vec<u32> {
    let mut ids: Vec<u32> = EVENT_ORDER
        .iter()
        .filter(|&&event| !flags.is_event_disabled(event))
        .map(|&event| event_type_id(event))
        .collect();
    ids.push(EOS_ID);
    ids
}

/// Allowed ids for slot `slot` (>= 1) of an event of type `kind`. Beyond the
/// event's parameter count only pad is allowed; the channel field drops disabled
/// channels.
pub fn allowed_param_ids(kind: EventType, slot: usize, flags: &DecodeFlags) -> Vec<u32> {
    let fields = kind.fields();
    if slot > fields.len() {
        return vec![PAD_ID];
    }
    let field = fields[slot - 1];
    let base = field_base(field);
    let ids = (0..field.size()).map(|value| base + value);
    if field == Field::Channel {
        ids.filter(|id| !flags.disabled_channel_ids.contains(id))
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
        let ids = allowed_event_ids(&flags);
        // note, key_signature, time_signature remain; patch/cc/tempo removed; eos present.
        assert!(ids.contains(&event_type_id(EventType::Note)));
        assert!(!ids.contains(&event_type_id(EventType::PatchChange)));
        assert!(!ids.contains(&event_type_id(EventType::ControlChange)));
        assert!(!ids.contains(&event_type_id(EventType::SetTempo)));
        assert!(ids.contains(&EOS_ID));
    }

    #[test]
    fn channel_slot_filters_disabled() {
        let flags = DecodeFlags::new(false, false, true, &[3]);
        // note slot 4 is the channel field.
        let ids = allowed_param_ids(EventType::Note, 4, &flags);
        assert!(!ids.contains(&field_token(Field::Channel, 3)));
        assert!(ids.contains(&field_token(Field::Channel, 0)));
    }

    #[test]
    fn beyond_params_is_pad() {
        let flags = DecodeFlags::new(false, false, true, &[]);
        // set_tempo has 4 params; slot 5 must be pad.
        assert_eq!(
            allowed_param_ids(EventType::SetTempo, 5, &flags),
            vec![PAD_ID]
        );
    }
}
