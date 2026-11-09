extern crate alloc;
use crate::{legends::KEYS, roster::Keyboard};
use alloc::vec::Vec;
use trueos::{
    input::{self, TrueosKeyboardOutputEvent},
    ui4_scene::InputRoute,
};

const TAP_MS: u64 = 90;
const KEY_COUNT: usize = 62;
struct Board {
    keyboard: Keyboard,
    held: [u32; 8],
    pulse_until: [u64; KEY_COUNT],
}
impl Board {
    fn new(keyboard: Keyboard) -> Self {
        Self {
            keyboard,
            held: [0; 8],
            pulse_until: [0; KEY_COUNT],
        }
    }
    fn any(&self) -> bool {
        self.held.iter().any(|&word| word != 0) || self.pulse_until.iter().any(|&until| until != 0)
    }
    fn reset(&mut self) -> bool {
        let changed = self.any();
        self.held = [0; 8];
        self.pulse_until = [0; KEY_COUNT];
        changed
    }
    fn down(&self, usage: u8) -> bool {
        self.held[usage as usize / 32] & (1 << (usage % 32)) != 0
    }
}

/// Input comes only from the frame's routed physical queue. Endpoint identity
/// follows roster reorder; typed characters and Shift do not remap physical keys.
#[derive(Default)]
pub struct Feedback {
    boards: Vec<Board>,
}
impl Feedback {
    pub fn set_roster(&mut self, keyboards: &[Keyboard]) {
        let mut previous = core::mem::take(&mut self.boards);
        self.boards = keyboards
            .iter()
            .map(|&keyboard| {
                previous
                    .iter()
                    .position(|b| {
                        b.keyboard.endpoint == keyboard.endpoint
                            && b.keyboard.combo_id == keyboard.combo_id
                    })
                    .map(|index| {
                        let mut b = previous.swap_remove(index);
                        b.keyboard = keyboard;
                        b
                    })
                    .unwrap_or_else(|| Board::new(keyboard))
            })
            .collect();
    }

    pub fn event(&mut self, event: &TrueosKeyboardOutputEvent, now: u64) -> bool {
        let endpoint = [event.controller_id, event.slot_id, event.ep_target];
        if event.kind == input::KEYBOARD_OUTPUT_KIND_DEVICE_LOST {
            let mut changed = false;
            for board in &mut self.boards {
                if endpoint == [0; 3] || board.keyboard.endpoint == endpoint {
                    changed |= board.reset();
                }
            }
            return changed;
        }
        if event.kind != input::KEYBOARD_OUTPUT_KIND_PHYSICAL {
            return false;
        }
        let Ok(usage) = u8::try_from(event.key_code) else {
            return false;
        };
        let Some(key) = KEYS.iter().position(|key| key.usage == Some(usage)) else {
            return false;
        };
        let Some(board) = self
            .boards
            .iter_mut()
            .find(|b| b.keyboard.endpoint == endpoint)
        else {
            return false;
        };
        let pressed = event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0;
        if board.down(usage) == pressed {
            return false;
        }
        let word = &mut board.held[usage as usize / 32];
        let bit = 1 << (usage % 32);
        if pressed {
            *word |= bit;
            board.pulse_until[key] = now.saturating_add(TAP_MS);
        } else {
            *word &= !bit;
        }
        true
    }

    pub fn expire(&mut self, now: u64) -> bool {
        let mut changed = false;
        for board in &mut self.boards {
            for (key, definition) in KEYS.iter().enumerate() {
                if board.pulse_until[key] != 0 && now >= board.pulse_until[key] {
                    board.pulse_until[key] = 0;
                    changed |= definition.usage.is_some_and(|usage| !board.down(usage));
                }
            }
        }
        changed
    }

    pub fn any(&self) -> bool {
        self.boards.iter().any(Board::any)
    }

    /// Per-cursor selection supports multiple combos selecting this window.
    /// Losing selection or the keyboard-bearing route clears its old presses.
    pub fn reconcile_selection(&mut self, routes: &[InputRoute]) -> bool {
        let mut changed = false;
        for board in &mut self.boards {
            let selected = routes.iter().any(|route| {
                route.selected_for_window
                    && route.keyboard.is_some_and(|k| {
                        (board.keyboard.combo_id != 0 && board.keyboard.combo_id == route.combo_id)
                            || [k.controller_id, k.slot_id, k.ep_target] == board.keyboard.endpoint
                    })
            });
            if !selected {
                changed |= board.reset();
            }
        }
        changed
    }

    pub fn active(&self, board: usize, key: usize) -> bool {
        self.boards.get(board).is_some_and(|b| {
            KEYS[key].usage.is_some_and(|usage| b.down(usage)) || b.pulse_until[key] != 0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keyboard(slot: u32) -> Keyboard {
        Keyboard {
            endpoint: [1, slot, 3],
            combo_id: slot,
            rgba: [128; 4],
        }
    }
    fn event(slot: u32, usage: u16, press: bool) -> TrueosKeyboardOutputEvent {
        TrueosKeyboardOutputEvent {
            controller_id: 1,
            slot_id: slot,
            ep_target: 3,
            kind: input::KEYBOARD_OUTPUT_KIND_PHYSICAL,
            key_code: usage,
            flags: if press {
                input::KEYBOARD_OUTPUT_FLAG_PRESS
            } else {
                0
            },
            ..Default::default()
        }
    }
    fn key(usage: u8) -> usize {
        KEYS.iter().position(|k| k.usage == Some(usage)).unwrap()
    }

    #[test]
    fn hid_identity_keeps_german_yz_shift_and_boards_independent() {
        let mut f = Feedback::default();
        f.set_roster(&[keyboard(1), keyboard(2)]);
        let mut z = event(1, 0x1c, true);
        z.codepoint = 'Y' as u32;
        z.modifiers = 2;
        assert!(f.event(&z, 100));
        assert!(f.active(0, key(0x1c)));
        assert!(!f.active(1, key(0x1c)));
        assert_eq!(KEYS[key(0x1c)].mesh, 34); // physical Y usage is printed Z
        assert_eq!(KEYS[key(0x1d)].mesh, 52); // physical Z usage is printed Y
        assert!(f.event(&event(2, 0xe1, true), 100)); // left Shift has its own marker
        assert!(f.active(1, key(0xe1)));
        assert!(!f.active(0, key(0xe1)));
        assert!(!f.event(&event(3, 4, true), 100)); // undisplayed source
        let text = TrueosKeyboardOutputEvent {
            kind: input::KEYBOARD_OUTPUT_KIND_TEXT,
            ..z
        };
        assert!(!f.event(&text, 100));
    }

    #[test]
    fn quick_taps_remain_visible_while_long_holds_release_without_timeout() {
        let mut f = Feedback::default();
        f.set_roster(&[keyboard(1)]);
        assert!(f.event(&event(1, 4, true), 100));
        assert!(!f.event(&event(1, 4, true), 110)); // repeat cannot extend the pulse
        assert!(f.event(&event(1, 4, false), 115));
        assert!(f.active(0, key(4)));
        assert!(!f.expire(189));
        assert!(f.expire(190));
        assert!(!f.active(0, key(4)));
        f.event(&event(1, 4, true), 200);
        assert!(!f.expire(1000));
        assert!(f.active(0, key(4)));
        f.event(&event(1, 4, false), 1001);
        assert!(!f.active(0, key(4)));
    }

    #[test]
    fn roster_reorder_preserves_source_and_removal_or_reset_cannot_leave_stuck_keys() {
        let mut f = Feedback::default();
        f.set_roster(&[keyboard(1), keyboard(2)]);
        f.event(&event(1, 4, true), 100);
        f.set_roster(&[keyboard(2), keyboard(1)]);
        assert!(!f.active(0, key(4)));
        assert!(f.active(1, key(4)));
        f.event(&event(2, 5, true), 100);
        let lost = TrueosKeyboardOutputEvent {
            kind: input::KEYBOARD_OUTPUT_KIND_DEVICE_LOST,
            ..event(1, 0, false)
        };
        assert!(f.event(&lost, 101));
        assert!(!f.active(1, key(4)));
        assert!(f.active(0, key(5)));
        f.set_roster(&[keyboard(1)]);
        assert!(!f.any());
        f.event(&event(1, 4, true), 102);
        assert!(f.event(
            &TrueosKeyboardOutputEvent {
                kind: input::KEYBOARD_OUTPUT_KIND_DEVICE_LOST,
                ..Default::default()
            },
            103
        ));
        assert!(!f.any());
    }

    #[test]
    fn per_combo_selection_clears_only_the_route_that_was_deselected() {
        use trueos::ui4_scene::{CursorSource, KeyboardState};
        let route = |slot| InputRoute {
            cursor: CursorSource {
                controller_id: 5,
                slot_id: slot,
                ep_target: 1,
                hid_kind: 2,
            },
            combo_id: slot,
            color_rgba: 0,
            selected_for_window: true,
            application_focus: false,
            vcursor: true,
            keyboard: Some(KeyboardState {
                controller_id: 1,
                slot_id: slot,
                ep_target: 3,
                combo_id: slot,
                modifiers: 0,
                source_kind: 0,
                virtual_keyboard: true,
                keys: [0; 6],
                ascii: [0; 6],
                key_down_bits: [0; 8],
            }),
        };
        let mut f = Feedback::default();
        f.set_roster(&[keyboard(1), keyboard(2)]);
        f.event(&event(1, 4, true), 100);
        f.event(&event(2, 5, true), 100);
        let mut routes = [route(1), route(2)];
        assert!(!f.reconcile_selection(&routes)); // independent of global app focus
        routes[0].selected_for_window = false;
        assert!(f.reconcile_selection(&routes));
        assert!(!f.active(0, key(4)));
        assert!(f.active(1, key(5)));
        routes[1].keyboard = None;
        assert!(f.reconcile_selection(&routes));
        assert!(!f.any());
    }

    #[test]
    fn keyfarm_covers_every_key_except_hardware_local_fn() {
        assert_eq!(KEYS.len(), KEY_COUNT);
        assert_eq!(KEYS.iter().filter(|k| k.usage.is_some()).count(), 61);
        assert_eq!(KEYS.iter().find(|k| k.usage.is_none()).unwrap().mesh, 25);
        for legend in crate::legends::LEGENDS {
            assert!(
                KEYS.iter()
                    .any(|key| key.mesh == legend.mesh && key.usage.is_some())
            );
        }
        assert_eq!(KEYS[key(0x65)].mesh, 26); // audited Menu after Fn
        assert_eq!(KEYS[key(0xe4)].mesh, 27); // audited right Ctrl
    }
}
