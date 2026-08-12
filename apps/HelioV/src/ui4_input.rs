//! TRUEOS UI4 adapter for Helio's platform-neutral navigation semantics.
//!
//! UI4 remains authoritative for device pairing, selection, capture, and
//! focus. A single camera consumes only the application-focused cursor/combo
//! route and its paired keyboard; other mice and keyboards are not merged.

use helio::{NavigationAction, NavigationInput, NavigationState};
use trueos::ui4_scene::{
    CursorSource, Error, Frame, InputRoute, KeyboardState, POINTER_BUTTON_PRIMARY,
};

const HID_A: u8 = 0x04;
const HID_D: u8 = 0x07;
const HID_S: u8 = 0x16;
const HID_W: u8 = 0x1a;
const HID_SPACE: u8 = 0x2c;
const HID_CONTROL_LEFT: u8 = 0xe0;
const HID_SHIFT_LEFT: u8 = 0xe1;
const HID_CONTROL_RIGHT: u8 = 0xe4;
const HID_SHIFT_RIGHT: u8 = 0xe5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RouteIdentity {
    cursor: CursorSource,
    combo_id: u32,
}

#[derive(Debug, Default)]
pub struct Ui4FlyInput {
    navigation: NavigationState,
    focused_route: Option<RouteIdentity>,
}

impl Ui4FlyInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sample one coherent UI4 routing snapshot, drain routed relative motion,
    /// and reduce both into engine-level navigation semantics.
    pub fn sample(&mut self, frame: &mut Frame) -> Result<NavigationInput, Error> {
        let routes = frame.input_routes()?;
        // `application_focus` names the singular focused frame and is repeated
        // on every cursor route. `selected_for_window` is the per-cursor half
        // of the contract. The focused held-key snapshot lets us recover the
        // most recently selecting cursor/combo when several cursors still
        // select this same frame.
        let focused_keyboard = if routes.iter().any(|route| route.application_focus) {
            frame.keyboard_state()?
        } else {
            None
        };
        let preferred =
            preferred_route(&routes, focused_keyboard, self.focused_route).map(RouteIdentity::from);
        self.focus_route(preferred);

        loop {
            let Some(event) = frame.take_pointer_event()? else {
                break;
            };
            let event_route = RouteIdentity {
                cursor: event.source,
                combo_id: event.combo_id,
            };
            if !route_is_eligible(&routes, event_route)
                || event.buttons_down & POINTER_BUTTON_PRIMARY == 0
            {
                continue;
            }

            // A fresh primary gesture is also an unambiguous route claim for
            // keyboard-less cursor devices. Never merge two local users into
            // the same camera state.
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0 {
                self.focus_route(Some(event_route));
            }
            if Some(event_route) == self.focused_route {
                self.navigation
                    .add_look_delta(glam::Vec2::new(event.dx as f32, event.dy as f32));
            }
        }

        let keyboard = self.focused_route.and_then(|identity| {
            routes
                .iter()
                .find(|route| RouteIdentity::from(*route) == identity)
                .and_then(|route| route.keyboard)
        });
        sync_keyboard(&mut self.navigation, keyboard);

        Ok(self.navigation.take_input())
    }

    fn focus_route(&mut self, route: Option<RouteIdentity>) {
        if route != self.focused_route {
            // Never carry held state or relative deltas across users/routes.
            self.navigation.set_focused(false);
            self.focused_route = route;
        }
        self.navigation.set_focused(route.is_some());
    }
}

impl From<&InputRoute> for RouteIdentity {
    fn from(route: &InputRoute) -> Self {
        Self {
            cursor: route.cursor,
            combo_id: route.combo_id,
        }
    }
}

fn preferred_route(
    routes: &[InputRoute],
    focused_keyboard: Option<KeyboardState>,
    current: Option<RouteIdentity>,
) -> Option<&InputRoute> {
    let eligible = |route: &&InputRoute| route.application_focus && route.selected_for_window;
    focused_keyboard
        .and_then(|keyboard| {
            routes
                .iter()
                .filter(eligible)
                .find(|route| route_keyboard_matches(route, keyboard))
        })
        .or_else(|| {
            current.and_then(|identity| {
                routes
                    .iter()
                    .filter(eligible)
                    .find(|route| RouteIdentity::from(*route) == identity)
            })
        })
        .or_else(|| routes.iter().find(eligible))
}

fn route_keyboard_matches(route: &InputRoute, keyboard: KeyboardState) -> bool {
    route.keyboard.is_some_and(|candidate| {
        candidate.controller_id == keyboard.controller_id
            && candidate.slot_id == keyboard.slot_id
            && candidate.ep_target == keyboard.ep_target
            && candidate.combo_id == keyboard.combo_id
    })
}

fn route_is_eligible(routes: &[InputRoute], identity: RouteIdentity) -> bool {
    routes.iter().any(|route| {
        route.application_focus
            && route.selected_for_window
            && RouteIdentity::from(route) == identity
    })
}

fn sync_keyboard(navigation: &mut NavigationState, keyboard: Option<KeyboardState>) {
    let down = |usage| keyboard.is_some_and(|state| state.is_down(usage));
    navigation.set(NavigationAction::MoveForward, down(HID_W));
    navigation.set(NavigationAction::MoveBackward, down(HID_S));
    navigation.set(NavigationAction::MoveLeft, down(HID_A));
    navigation.set(NavigationAction::MoveRight, down(HID_D));
    navigation.set(NavigationAction::MoveUp, down(HID_SPACE));
    navigation.set(
        NavigationAction::MoveDown,
        down(HID_SHIFT_LEFT) || down(HID_SHIFT_RIGHT),
    );
    navigation.set(
        NavigationAction::Boost,
        down(HID_CONTROL_LEFT) || down(HID_CONTROL_RIGHT),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(slot_id: u32) -> CursorSource {
        CursorSource {
            controller_id: 1,
            slot_id,
            ep_target: 3,
            hid_kind: 4,
        }
    }

    fn keyboard_with(usages: &[u8]) -> KeyboardState {
        let mut state = KeyboardState {
            controller_id: 1,
            slot_id: 2,
            ep_target: 3,
            combo_id: 4,
            modifiers: 0,
            source_kind: 0,
            virtual_keyboard: false,
            keys: [0; 6],
            ascii: [0; 6],
            key_down_bits: [0; 8],
        };
        for usage in usages {
            state.key_down_bits[*usage as usize / 32] |= 1 << (*usage as usize % 32);
        }
        state
    }

    fn route(
        slot_id: u32,
        selected_for_window: bool,
        application_focus: bool,
        keyboard: Option<KeyboardState>,
    ) -> InputRoute {
        InputRoute {
            cursor: cursor(slot_id),
            combo_id: keyboard.map_or(0, |state| state.combo_id),
            color_rgba: 0,
            selected_for_window,
            application_focus,
            vcursor: false,
            keyboard,
        }
    }

    #[test]
    fn ui4_hid_snapshot_maps_to_shared_actions() {
        let mut navigation = NavigationState::new();
        sync_keyboard(
            &mut navigation,
            Some(keyboard_with(&[HID_W, HID_D, HID_SPACE, HID_CONTROL_LEFT])),
        );
        let input = navigation.take_input();
        assert_eq!(input.movement, glam::Vec3::new(1.0, 1.0, 1.0));
        assert!(input.boost);
    }

    #[test]
    fn missing_keyboard_releases_all_actions() {
        let mut navigation = NavigationState::new();
        sync_keyboard(&mut navigation, Some(keyboard_with(&[HID_W])));
        sync_keyboard(&mut navigation, None);
        assert_eq!(navigation.take_input().movement, glam::Vec3::ZERO);
    }

    #[test]
    fn frame_wide_focus_does_not_choose_an_unselected_cursor() {
        let routes = [route(1, false, true, None), route(2, true, true, None)];
        let selected = preferred_route(&routes, None, None).expect("selected route");
        assert_eq!(selected.cursor, cursor(2));
    }

    #[test]
    fn focused_keyboard_disambiguates_selected_cursor_routes() {
        let mut first_keyboard = keyboard_with(&[HID_W]);
        first_keyboard.slot_id = 10;
        let mut second_keyboard = keyboard_with(&[HID_D]);
        second_keyboard.slot_id = 20;
        let routes = [
            route(1, true, true, Some(first_keyboard)),
            route(2, true, true, Some(second_keyboard)),
        ];
        let selected =
            preferred_route(&routes, Some(second_keyboard), None).expect("keyboard-matched route");
        assert_eq!(selected.cursor, cursor(2));
    }
}
