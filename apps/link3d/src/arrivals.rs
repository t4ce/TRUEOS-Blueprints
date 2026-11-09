//! Late arrivals reuse the startup slide motion on shared PBR instance rows.
//! Source class + endpoint identify a device; palette, bucket and roster order
//! changes do not turn an existing device into a new arrival.
extern crate alloc;
use crate::{
    intro::{LANDING_MS, round_robin, slide_offset},
    roster::{Keyboard, Mouse, Tablet},
};
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    kind: u8,
    endpoint: [u32; 3],
}
struct Flight {
    device: Identity,
    started: u64,
}
#[derive(Default)]
pub struct Arrivals {
    devices: Vec<Identity>,
    flights: Vec<Flight>,
    dirty: bool,
}
impl Arrivals {
    pub fn configure(
        &mut self,
        keyboards: &[Keyboard],
        mice: &[Mouse],
        tablets: &[Tablet],
        now: u64,
        animate: bool,
    ) {
        let current: Vec<_> = keyboards
            .iter()
            .map(|d| {
                (
                    Identity {
                        kind: 0,
                        endpoint: d.endpoint,
                    },
                    d.combo_id,
                )
            })
            .chain(mice.iter().map(|d| {
                (
                    Identity {
                        kind: 1,
                        endpoint: d.endpoint,
                    },
                    d.combo_id,
                )
            }))
            .chain(tablets.iter().map(|d| {
                (
                    Identity {
                        kind: 2,
                        endpoint: d.endpoint,
                    },
                    d.combo_id,
                )
            }))
            .collect();
        self.flights
            .retain(|flight| current.iter().any(|(id, _)| *id == flight.device));
        if animate {
            let new: Vec<_> = current
                .iter()
                .copied()
                .filter(|(id, _)| !self.devices.contains(id))
                .collect();
            let combos: Vec<_> = new.iter().map(|(_, combo)| *combo).collect();
            for (rank, index) in round_robin(&combos).into_iter().enumerate() {
                self.flights.push(Flight {
                    device: new[index].0,
                    started: now.saturating_add(rank as u64 * 120),
                });
            }
        } else {
            self.flights.clear();
        }
        self.devices = current.into_iter().map(|(id, _)| id).collect();
        self.dirty = true;
    }

    /// Includes one last all-zero sample so the GPU receives exact final rows.
    pub fn sample(&mut self, now: u64, radius: f32) -> Option<Vec<f32>> {
        if !self.dirty && self.flights.is_empty() {
            return None;
        }
        let offsets = self
            .devices
            .iter()
            .map(|id| {
                self.flights
                    .iter()
                    .find(|f| f.device == *id)
                    .map_or(0., |flight| {
                        slide_offset(now.saturating_sub(flight.started) as f32, radius)
                    })
            })
            .collect();
        self.flights
            .retain(|flight| now < flight.started.saturating_add(LANDING_MS as u64));
        self.dirty = false;
        Some(offsets)
    }
    pub fn skip(&mut self) {
        if !self.flights.is_empty() {
            self.dirty = true;
        }
        self.flights.clear();
    }
}

/// Preserve the original seed order, draw groups, palette columns and all other
/// TRS data. Only X translation changes; no extra copy of the mesh is needed.
pub fn translated_seeds(seeds: &[u8], offsets: &[f32]) -> Vec<u8> {
    assert_eq!(seeds.len(), offsets.len() * 64);
    let mut moved = seeds.to_vec();
    for (row, &offset) in moved.chunks_exact_mut(64).zip(offsets) {
        if offset == 0. {
            continue;
        }
        for field in [0, 44] {
            let x = f32::from_le_bytes(row[field..field + 4].try_into().unwrap());
            row[field..field + 4].copy_from_slice(&(x + offset).to_le_bytes());
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::prepare_row;
    fn keyboard(slot: u32, combo_id: u32) -> Keyboard {
        Keyboard {
            endpoint: [1, slot, 2],
            combo_id,
            rgba: [32, 168, 255, 255],
        }
    }
    #[test]
    fn late_keyboard_slides_but_stationary_and_recolored_devices_do_not_replay() {
        let old = keyboard(2, 7);
        let new = keyboard(1, 8);
        let mut arrivals = Arrivals::default();
        arrivals.configure(&[old], &[], &[], 0, false);
        assert_eq!(arrivals.sample(0, 20.).unwrap(), [0.]);
        assert!(arrivals.sample(1, 20.).is_none());
        arrivals.configure(&[new, old], &[], &[], 1_000, true);
        assert_eq!(arrivals.sample(1_000, 20.).unwrap(), [100., 0.]);
        let half = arrivals.sample(1_325, 20.).unwrap();
        assert!(half[0] > 0. && half[0] < 100.);
        assert_eq!(half[1], 0.);
        let mut moved = new;
        moved.combo_id = 9;
        moved.rgba = [255, 64, 64, 255];
        arrivals.configure(&[old, moved], &[], &[], 1_325, true);
        assert_eq!(arrivals.sample(1_325, 20.).unwrap(), [0., half[0]]);
        assert_eq!(arrivals.sample(1_650, 20.).unwrap(), [0., 0.]);
        assert!(arrivals.sample(1_651, 20.).is_none());
    }
    #[test]
    fn on_demand_remote_pointer_and_keyboard_have_independent_identity_and_round_robin_turns() {
        let mut arrivals = Arrivals::default();
        let mouse = Mouse {
            endpoint: [9, 1, 0],
            combo_id: 2,
            rgba: [1; 4],
        };
        let tablet = Tablet {
            endpoint: mouse.endpoint,
            combo_id: 2,
            rgba: [1; 4],
        };
        arrivals.configure(&[], &[mouse], &[], 0, false);
        arrivals.sample(0, 20.);
        let keys = [keyboard(1, 1), keyboard(2, 2)];
        arrivals.configure(&keys, &[mouse], &[tablet], 1_000, true);
        let first = arrivals.sample(1_120, 20.).unwrap();
        assert!(first[0] < 100.);
        assert_eq!(first[1], 100.);
        assert_eq!(first[2], 0.); // Existing mouse shares the tablet endpoint.
        assert_eq!(first[3], 100.);
        // Removing an in-flight device cancels it; re-adding the endpoint slides anew.
        arrivals.configure(&keys, &[mouse], &[], 1_200, true);
        arrivals.configure(&keys, &[mouse], &[tablet], 2_000, true);
        assert_eq!(arrivals.sample(2_000, 20.).unwrap(), [0., 0., 0., 100.]);
        arrivals.skip();
        assert_eq!(arrivals.sample(2_001, 20.).unwrap(), [0.; 4]);
        assert!(arrivals.sample(2_002, 20.).is_none());
    }
    #[test]
    fn slide_upload_keeps_palette_indices_and_shared_geometry_transforms_intact() {
        let row = prepare_row(&[keyboard(1, 1), keyboard(2, 2)], 20.);
        let moved = translated_seeds(&row.seeds, &[0., 10.]);
        assert_eq!(&moved[..64], &row.seeds[..64]);
        assert_eq!(&moved[68..108], &row.seeds[68..108]);
        assert_eq!(&moved[112..], &row.seeds[112..]);
        for start in [64, 108] {
            assert_eq!(
                f32::from_le_bytes(moved[start..start + 4].try_into().unwrap()),
                10.
            );
        }
        assert_eq!(translated_seeds(&row.seeds, &[0.; 2]), row.seeds);
    }
}
