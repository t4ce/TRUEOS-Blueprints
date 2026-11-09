extern crate alloc;

use alloc::vec::Vec;
use trueos::hid::{
    InputComboColor, TrueosHidHutKeyboardState, TrueosHidHutMouseState, TrueosInputCombo,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keyboard {
    pub endpoint: [u32; 3],
    pub combo_id: u32,
    pub rgba: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mouse {
    pub endpoint: [u32; 3],
    pub combo_id: u32,
    pub rgba: [u8; 4],
}

fn combo_rgba(combo_id: u32, combos: &[TrueosInputCombo]) -> [u8; 4] {
    combos
        .iter()
        .find(|combo| combo_id != 0 && combo.combo_id == combo_id)
        .map_or([128, 128, 128, 255], |combo| {
            InputComboColor::from_index(combo.color_id).rgba()
        })
}

/// Physical and virtual mice use the same HUT identities and combo buckets as
/// Link. Position and button state do not change this static scene roster.
pub fn mice(devices: &[TrueosHidHutMouseState], combos: &[TrueosInputCombo]) -> Vec<Mouse> {
    let mut roster: Vec<_> = devices
        .iter()
        .map(|mouse| Mouse {
            endpoint: [mouse.controller_id, mouse.slot_id, mouse.ep_target],
            combo_id: mouse.combo_id,
            rgba: combo_rgba(mouse.combo_id, combos),
        })
        .collect();
    roster.sort_unstable_by_key(|mouse| mouse.endpoint);
    roster
}

/// Project the live HUT roster into stable visual identities. Key activity is
/// deliberately absent so typing does not trigger a scene rebuild.
pub fn virtual_keyboards(
    keyboards: &[TrueosHidHutKeyboardState],
    combos: &[TrueosInputCombo],
) -> Vec<Keyboard> {
    let mut roster: Vec<_> = keyboards
        .iter()
        .filter(|keyboard| keyboard.virtual_device != 0)
        .map(|keyboard| Keyboard {
            endpoint: [keyboard.controller_id, keyboard.slot_id, keyboard.ep_target],
            combo_id: keyboard.combo_id,
            rgba: combo_rgba(keyboard.combo_id, combos),
        })
        .collect();
    roster.sort_unstable_by_key(|keyboard| keyboard.endpoint);
    roster
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard(slot_id: u32, virtual_device: bool, combo_id: u32) -> TrueosHidHutKeyboardState {
        TrueosHidHutKeyboardState {
            controller_id: 1,
            slot_id,
            ep_target: 2,
            combo_id,
            virtual_device: u8::from(virtual_device),
            ..TrueosHidHutKeyboardState::default()
        }
    }

    fn combo(combo_id: u32, color_id: u8) -> TrueosInputCombo {
        TrueosInputCombo {
            combo_id,
            color_id,
            ..TrueosInputCombo::default()
        }
    }

    #[test]
    fn mice_include_physical_and_virtual_sources_without_tracking_motion() {
        let mut devices = [
            TrueosHidHutMouseState {
                controller_id: 1,
                slot_id: 7,
                ep_target: 2,
                combo_id: 1,
                ..TrueosHidHutMouseState::default()
            },
            TrueosHidHutMouseState {
                controller_id: 1,
                slot_id: 3,
                ep_target: 2,
                combo_id: 2,
                virtual_device: 1,
                ..TrueosHidHutMouseState::default()
            },
        ];
        let combos = [
            combo(1, InputComboColor::Cyan as u8),
            combo(2, InputComboColor::Violet as u8),
        ];
        let initial = mice(&devices, &combos);
        assert_eq!(initial.len(), 2);
        assert_eq!(initial[0].endpoint, [1, 3, 2]);
        assert_eq!(initial[0].rgba, InputComboColor::Violet.rgba());
        assert_eq!(initial[1].rgba, InputComboColor::Cyan.rgba());
        devices.reverse();
        devices[0].x = 1234.;
        devices[0].y = -99.;
        devices[0].buttons_down = 7;
        assert_eq!(mice(&devices, &combos), initial);
        devices[0].combo_id = 1;
        assert_eq!(mice(&devices, &combos)[0].rgba, initial[1].rgba);
        assert_eq!(mice(&devices[..1], &combos).len(), 1);
        assert_eq!(mice(&devices, &[])[0].rgba, [128, 128, 128, 255]);
        assert!(mice(&[], &combos).is_empty());
    }

    #[test]
    fn empty_roster_and_empty_combos_never_create_placeholder_keyboards() {
        assert!(virtual_keyboards(&[], &[]).is_empty());
        assert!(virtual_keyboards(&[], &[combo(1, 2)]).is_empty());
        assert!(virtual_keyboards(&[keyboard(1, false, 1)], &[combo(1, 2)]).is_empty());
    }

    #[test]
    fn device_flag_selects_virtual_keyboards_independently_of_persona() {
        let mut human_virtual = keyboard(3, true, 0);
        human_virtual.source_kind = trueos::hid::InputComboSourceKind::Human as u8;
        let mut ai_physical = keyboard(2, false, 0);
        ai_physical.source_kind = trueos::hid::InputComboSourceKind::Ai as u8;
        let roster = virtual_keyboards(&[human_virtual, ai_physical], &[]);
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].endpoint, [1, 3, 2]);
        assert_eq!(roster[0].rgba, [128, 128, 128, 255]);
    }

    #[test]
    fn all_thirty_two_devices_survive_and_order_is_independent_of_snapshot_order() {
        let mut keyboards: Vec<_> = (1..=32).rev().map(|slot| keyboard(slot, true, 0)).collect();
        let roster = virtual_keyboards(&keyboards, &[]);
        assert_eq!(roster.len(), 32);
        for (index, entry) in roster.iter().enumerate() {
            assert_eq!(entry.endpoint, [1, index as u32 + 1, 2]);
        }
        keyboards.reverse();
        assert_eq!(virtual_keyboards(&keyboards, &[]), roster);
        keyboards[0].keys = [4, 5, 0, 0, 0, 0];
        keyboards[0].modifiers = 2;
        keyboards[0].key_down_bits[0] = 0x30;
        assert_eq!(virtual_keyboards(&keyboards, &[]), roster);
    }

    #[test]
    fn every_combo_color_uses_the_shared_palette() {
        let keyboards: Vec<_> = (0..InputComboColor::COUNT)
            .map(|color| keyboard(u32::from(color) + 1, true, u32::from(color) + 1))
            .collect();
        let combos: Vec<_> = (0..InputComboColor::COUNT)
            .map(|color| combo(u32::from(color) + 1, color))
            .collect();
        for (index, entry) in virtual_keyboards(&keyboards, &combos).iter().enumerate() {
            assert_eq!(entry.rgba, InputComboColor::from_index(index as u8).rgba());
        }
    }

    #[test]
    fn membership_color_updates_and_removals_follow_each_snapshot() {
        let mut device = keyboard(7, true, 1);
        let mut combos = [combo(1, 2), combo(2, 6)];
        let initial = virtual_keyboards(&[device], &combos)[0];
        device.combo_id = 2;
        let moved = virtual_keyboards(&[device], &combos)[0];
        assert_eq!(moved.endpoint, initial.endpoint);
        assert_eq!(moved.combo_id, 2);
        assert_eq!(moved.rgba, InputComboColor::Cyan.rgba());
        combos[1].color_id = InputComboColor::Violet as u8;
        assert_eq!(
            virtual_keyboards(&[device], &combos)[0].rgba,
            InputComboColor::Violet.rgba()
        );
        assert_eq!(
            virtual_keyboards(&[device], &[])[0].rgba,
            [128, 128, 128, 255]
        );
        device.combo_id = 0;
        assert_eq!(
            virtual_keyboards(&[device], &[combo(0, 2)])[0].rgba,
            [128, 128, 128, 255]
        );
        assert!(virtual_keyboards(&[], &combos).is_empty());
    }
}
