//! HelioC-local UI4 input-broker adapter.

use helio::{NavigationAction, NavigationInput, NavigationState};
use trueos::ui4_scene::{
    CursorSource, Error, Frame, InputRoute, KeyboardState, POINTER_BUTTON_PRIMARY,
    POINTER_BUTTON_SECONDARY,
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
pub(crate) struct Ui4FlyInput {
    navigation: NavigationState,
    focused_route: Option<RouteIdentity>,
}

impl Ui4FlyInput {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn sample(&mut self, frame: &mut Frame) -> Result<(NavigationInput, usize), Error> {
        let routes = frame.input_routes()?;
        let route_count = routes.len();
        let focused_keyboard = if routes.iter().any(|route| route.application_focus) {
            frame.keyboard_state()?
        } else {
            None
        };
        let preferred = focused_keyboard
            .and_then(|keyboard| {
                routes
                    .iter()
                    .filter(|route| route.application_focus && route.selected_for_window)
                    .find(|route| route_keyboard_matches(route, keyboard))
            })
            .or_else(|| {
                self.focused_route.and_then(|identity| {
                    routes.iter().find(|route| {
                        route.application_focus
                            && route.selected_for_window
                            && RouteIdentity::from(*route) == identity
                    })
                })
            })
            .or_else(|| {
                routes
                    .iter()
                    .find(|route| route.application_focus && route.selected_for_window)
            })
            .map(RouteIdentity::from);
        if preferred != self.focused_route {
            self.navigation.set_focused(false);
            self.focused_route = preferred;
        }
        self.navigation.set_focused(preferred.is_some());

        while let Some(event) = frame.take_pointer_event()? {
            let identity = RouteIdentity {
                cursor: event.source,
                combo_id: event.combo_id,
            };
            let eligible = routes.iter().any(|route| {
                route.application_focus
                    && route.selected_for_window
                    && RouteIdentity::from(route) == identity
            });
            if !eligible
                || event.buttons_down & POINTER_BUTTON_PRIMARY == 0
                || event.buttons_down & POINTER_BUTTON_SECONDARY != 0
            {
                continue;
            }
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0 {
                self.focused_route = Some(identity);
                self.navigation.set_focused(true);
            }
            if Some(identity) == self.focused_route {
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
        Ok((self.navigation.take_input(), route_count))
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

fn route_keyboard_matches(route: &InputRoute, keyboard: KeyboardState) -> bool {
    route.keyboard.is_some_and(|candidate| {
        candidate.controller_id == keyboard.controller_id
            && candidate.slot_id == keyboard.slot_id
            && candidate.ep_target == keyboard.ep_target
            && candidate.combo_id == keyboard.combo_id
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
