extern crate alloc;

use alloc::vec::Vec;
use trueos::ui4_scene::{CursorSource, InputRoute, PointerEvent};

/// Retain each cursor's contribution independently of event arrival order.
/// Selection comes from UI4, including stationary and keyboard-less cursors.
#[derive(Default)]
pub struct SharedTilt {
    positions: Vec<(CursorSource, [i32; 2])>,
}

impl SharedTilt {
    pub fn pointer(&mut self, event: &PointerEvent) {
        let source = event.source;
        let position = [event.local_x, event.local_y];
        if let Some(entry) = self.positions.iter_mut().find(|entry| entry.0 == source) {
            entry.1 = position;
        } else {
            self.positions.push((source, position));
        }
    }

    pub fn clear(&mut self) {
        self.positions.clear();
    }

    pub fn reconcile(&mut self, routes: &[InputRoute], extent: [u32; 2]) -> [f32; 2] {
        self.positions.retain(|(source, _)| {
            routes
                .iter()
                .any(|route| route.selected_for_window && route.cursor == *source)
        });
        let mut total = [0.; 2];
        let mut count = 0;
        for route in routes.iter().filter(|route| route.selected_for_window) {
            count += 1;
            if let Some((_, position)) = self.positions.iter().find(|entry| entry.0 == route.cursor)
            {
                for axis in 0..2 {
                    // Clamp each cursor before sharing the budget, so a cursor
                    // outside the frame cannot outweigh the other selectors.
                    total[axis] += (position[axis] as f32 * 2. / extent[axis].max(1) as f32 - 1.)
                        .clamp(-1., 1.);
                }
            }
            // An unseen position is neutral but still owns its equal share.
        }
        if count != 0 {
            total = total.map(|value| value / count as f32);
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(slot: u32, selected: bool) -> InputRoute {
        InputRoute {
            cursor: CursorSource {
                controller_id: 1,
                slot_id: slot,
                ep_target: 2,
                hid_kind: 3,
            },
            combo_id: slot,
            color_rgba: 0,
            selected_for_window: selected,
            application_focus: false,
            vcursor: false,
            keyboard: None,
        }
    }
    fn event(slot: u32, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            source: route(slot, true).cursor,
            x: 0,
            y: 0,
            local_x: x,
            local_y: y,
            dx: 0,
            dy: 0,
            wheel: 0,
            buttons_down: 0,
            buttons_pressed: 0,
            buttons_released: 0,
            combo_id: slot,
            vcursor: false,
        }
    }

    #[test]
    fn selectors_share_both_axes_and_stationary_cursors_keep_their_contribution() {
        let mut tilt = SharedTilt::default();
        tilt.pointer(&event(1, 800, 0));
        assert_eq!(tilt.reconcile(&[route(1, true)], [800, 600]), [1., -1.]);
        assert_eq!(
            tilt.reconcile(&[route(1, true), route(2, true)], [800, 600]),
            [0.5, -0.5]
        );
        tilt.pointer(&event(2, 0, 600));
        assert_eq!(
            tilt.reconcile(&[route(1, true), route(2, true)], [800, 600]),
            [0., 0.]
        );
        tilt.pointer(&event(2, 400, 300));
        assert_eq!(
            tilt.reconcile(&[route(2, true), route(1, true)], [800, 600]),
            [0.5, -0.5]
        );
        // A nonselected cursor has no vote, even if it has input focus.
        let mut unselected = route(3, false);
        unselected.application_focus = true;
        tilt.pointer(&event(3, i32::MAX, i32::MIN));
        assert_eq!(
            tilt.reconcile(&[route(1, true), route(2, true), unselected], [800, 600]),
            [0.5, -0.5]
        );
    }

    #[test]
    fn selection_changes_reweight_without_motion_and_remove_stale_positions() {
        let mut tilt = SharedTilt::default();
        tilt.pointer(&event(1, i32::MAX, i32::MAX));
        tilt.pointer(&event(2, 0, 0));
        assert_eq!(
            tilt.reconcile(&[route(1, true), route(2, true)], [800, 600]),
            [0., 0.]
        );
        assert_eq!(
            tilt.reconcile(&[route(1, true), route(2, false)], [800, 600]),
            [1., 1.]
        );
        assert_eq!(tilt.reconcile(&[route(2, true)], [800, 600]), [0., 0.]);
        tilt.pointer(&event(2, 800, 600));
        assert_eq!(tilt.reconcile(&[route(2, true)], [800, 600]), [1., 1.]);
        tilt.clear();
        assert_eq!(tilt.reconcile(&[route(2, true)], [800, 600]), [0., 0.]);
        assert_eq!(tilt.reconcile(&[], [0, 0]), [0., 0.]);
    }
}
