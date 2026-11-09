//! A one-shot entrance. Only small projected line buffers change during the fly-in;
//! the resident PBR mesh and its final instance transforms stay untouched.
extern crate alloc;
use crate::{
    roster::{Keyboard, Mouse, Tablet},
    scene::RowInstances,
};
use alloc::{vec, vec::Vec};
use trueos::vgpu::RetainedCamera;

include!(concat!(env!("OUT_DIR"), "/keyboard.outlines.rs"));
pub const FLIGHT_MS: u64 = 4_000;
pub const REVEAL_MS: u64 = 1_500;
const DELAY_MS: f32 = 200.;
pub(crate) const LANDING_MS: f32 = 650.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Waiting,
    Flying { elapsed: u64, zoom: f32 },
    Revealing { amount: f32 },
    Complete,
}
impl Stage {
    pub fn outline_alpha(self) -> f32 {
        match self {
            Self::Flying { .. } => 1.,
            Self::Revealing { amount } => 1. - amount,
            _ => 0.,
        }
    }
}

struct Device {
    position: [f32; 3],
    scale: [f32; 3],
    keyboard: bool,
    color: [u8; 4],
    rank: usize,
}
#[derive(Default)]
pub struct Intro {
    started: Option<u64>,
    finished: bool,
    devices: Vec<Device>,
    radius: f32,
}
impl Intro {
    pub fn configure(
        &mut self,
        row: &RowInstances,
        keyboards: &[Keyboard],
        mice: &[Mouse],
        tablets: &[Tablet],
    ) {
        if self.finished {
            return;
        }
        let combos: Vec<_> = keyboards
            .iter()
            .map(|d| d.combo_id)
            .chain(mice.iter().map(|d| d.combo_id))
            .chain(tablets.iter().map(|d| d.combo_id))
            .collect();
        let colors = keyboards
            .iter()
            .map(|d| d.rgba)
            .chain(mice.iter().map(|d| d.rgba))
            .chain(tablets.iter().map(|d| d.rgba));
        let order = round_robin(&combos);
        let mut ranks = vec![0; combos.len()];
        for (rank, &index) in order.iter().enumerate() {
            ranks[index] = rank;
        }
        self.radius = row.radius;
        self.devices = row
            .seeds
            .chunks_exact(64)
            .zip(colors)
            .enumerate()
            .map(|(index, (seed, color))| {
                let read = |i| f32::from_le_bytes(seed[i * 4..i * 4 + 4].try_into().unwrap());
                Device {
                    position: core::array::from_fn(read),
                    scale: core::array::from_fn(|i| read(i + 3)),
                    keyboard: index < keyboards.len(),
                    color,
                    rank: ranks[index],
                }
            })
            .collect();
    }
    pub fn stage(&self, now: u64) -> Stage {
        if self.finished || self.devices.is_empty() {
            return Stage::Complete;
        }
        let Some(started) = self.started else {
            return Stage::Waiting;
        };
        let elapsed = now.saturating_sub(started);
        if elapsed < FLIGHT_MS {
            Stage::Flying {
                elapsed,
                // Keep zoom moving throughout the arrivals instead of
                // spending most of the distance in the first half.
                zoom: smooth(elapsed as f32 / FLIGHT_MS as f32),
            }
        } else if elapsed < FLIGHT_MS + REVEAL_MS {
            Stage::Revealing {
                amount: smooth((elapsed - FLIGHT_MS) as f32 / REVEAL_MS as f32),
            }
        } else {
            Stage::Complete
        }
    }
    /// Start after publishing an empty frame and warming the shared font lane.
    pub fn start_if_ready(&mut self, now: u64, ready: bool) -> bool {
        if ready && !self.finished && !self.devices.is_empty() && self.started.is_none() {
            self.started = Some(now);
            true
        } else {
            false
        }
    }
    pub fn running(&self) -> bool {
        !self.finished && self.started.is_some()
    }
    pub fn finish(&mut self) {
        self.finished = true;
        self.devices.clear();
    }
    pub fn skip(&mut self) {
        self.finish();
    }

    /// NDC vertices, grouped by device so the existing immediate line pipeline
    /// can give each bucket its central palette color. No filled primitives.
    pub fn lines(
        &self,
        stage: Stage,
        camera: &RetainedCamera,
        half: [f32; 3],
        live_offsets: &[f32],
    ) -> (Vec<u8>, Vec<LineDraw>) {
        let mut vertices = Vec::new();
        let mut draws = Vec::new();
        for (index, device) in self.devices.iter().enumerate() {
            let offset = match stage {
                Stage::Flying { elapsed, .. } => {
                    let Some(offset) =
                        fly_offset(elapsed, device.rank, self.devices.len(), self.radius)
                    else {
                        continue;
                    };
                    offset
                }
                Stage::Revealing { .. } => 0.,
                _ => continue,
            };
            // A device discovered during the startup sequence still gets a
            // complete arrival, even if the global fly-in is almost finished.
            let offset = offset.max(live_offsets.get(index).copied().unwrap_or(0.));
            let first = (vertices.len() / 12) as u32;
            let pointer = [half.map(|v| -v), half];
            let boxes = if device.keyboard {
                KEYBOARD_BOXES
            } else {
                core::slice::from_ref(&pointer)
            };
            for &[min, max] in boxes {
                let corners: [[f32; 3]; 8] = core::array::from_fn(|corner| {
                    core::array::from_fn(|axis| {
                        let local = if corner & (1 << axis) == 0 {
                            min[axis]
                        } else {
                            max[axis]
                        };
                        local * device.scale[axis]
                            + device.position[axis]
                            + if axis == 0 { offset } else { 0. }
                    })
                });
                for corner in 0..8 {
                    for axis in 0..3 {
                        if corner & (1 << axis) != 0 {
                            continue;
                        }
                        if let Some(line) =
                            clip_line(camera, corners[corner], corners[corner | (1 << axis)])
                        {
                            for point in line {
                                for coordinate in point {
                                    vertices.extend_from_slice(&coordinate.to_le_bytes());
                                }
                            }
                        }
                    }
                }
            }
            let count = (vertices.len() / 12) as u32 - first;
            if count != 0 {
                let mut color = device.color;
                color[3] = libm::roundf(255. * stage.outline_alpha()) as u8;
                draws.push(LineDraw {
                    first,
                    count,
                    color,
                });
            }
        }
        (vertices, draws)
    }
}

pub struct LineDraw {
    pub first: u32,
    pub count: u32,
    pub color: [u8; 4],
}

pub(crate) fn round_robin(combos: &[u32]) -> Vec<usize> {
    let mut buckets: Vec<(u32, Vec<usize>)> = Vec::new();
    for (index, &combo) in combos.iter().enumerate() {
        // Unassigned devices have no common combo and each gets its own turn.
        if combo != 0
            && let Some((_, devices)) = buckets.iter_mut().find(|(id, _)| *id == combo)
        {
            devices.push(index);
        } else {
            buckets.push((combo, vec![index]));
        }
    }
    let mut order = Vec::with_capacity(combos.len());
    let mut turn = 0;
    while order.len() < combos.len() {
        for (_, devices) in &buckets {
            if let Some(&index) = devices.get(turn) {
                order.push(index);
            }
        }
        turn += 1;
    }
    order
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn ease(t: f32) -> f32 {
    let inverse = 1. - t.clamp(0., 1.);
    1. - inverse * inverse * inverse
}
fn fly_offset(elapsed: u64, rank: usize, count: usize, radius: f32) -> Option<f32> {
    let stagger =
        (FLIGHT_MS as f32 - DELAY_MS - LANDING_MS) / count.saturating_sub(1).max(1) as f32;
    let start = DELAY_MS + rank as f32 * stagger;
    if (elapsed as f32) < start {
        return None;
    }
    Some(slide_offset(elapsed as f32 - start, radius))
}

/// Shared right-to-left arrival motion for the startup outlines and live devices.
pub(crate) fn slide_offset(elapsed_ms: f32, radius: f32) -> f32 {
    radius * 5. * (1. - ease(elapsed_ms / LANDING_MS))
}

/// Clip in homogeneous space before dividing, including the near plane. This
/// matters while the intro camera is very close and devices enter offscreen.
fn clip_line(camera: &RetainedCamera, a: [f32; 3], b: [f32; 3]) -> Option<[[f32; 3]; 2]> {
    let transform = |point: [f32; 3]| -> [f32; 4] {
        core::array::from_fn(|row| {
            camera.view_projection[12 + row]
                + (0..3)
                    .map(|axis| camera.view_projection[axis * 4 + row] * point[axis])
                    .sum::<f32>()
        })
    };
    let a = transform(a);
    let b = transform(b);
    if a.iter().chain(&b).any(|v| !v.is_finite()) {
        return None;
    }
    let planes = |p: [f32; 4]| {
        [
            p[3] + p[0],
            p[3] - p[0],
            p[3] + p[1],
            p[3] - p[1],
            p[2],
            p[3] - p[2],
        ]
    };
    let (mut enter, mut leave) = (0f32, 1f32);
    for (start, end) in planes(a).into_iter().zip(planes(b)) {
        if start < 0. && end < 0. {
            return None;
        }
        if start < 0. {
            enter = enter.max(start / (start - end));
        }
        if end < 0. {
            leave = leave.min(start / (start - end));
        }
    }
    if enter > leave {
        return None;
    }
    let points = [enter, leave]
        .map(|t| -> [f32; 4] { core::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t) });
    if points.iter().any(|point| point[3] <= 0.) {
        return None;
    }
    Some(points.map(|p| [p[0] / p[3], p[1] / p[3], p[2] / p[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrbitCamera, scene::prepare_row};
    fn keyboards() -> [Keyboard; 2] {
        core::array::from_fn(|i| Keyboard {
            endpoint: [1, i as u32, 0],
            combo_id: i as u32 + 1,
            rgba: [255, 20, 30, 255],
        })
    }
    #[test]
    fn buckets_take_turns_and_unassigned_devices_are_independent() {
        assert_eq!(round_robin(&[1, 2, 1, 3, 2, 3, 1]), [0, 1, 3, 2, 4, 5, 6]);
        assert_eq!(round_robin(&[0, 1, 0, 1]), [0, 1, 2, 3]);
        assert!(round_robin(&[]).is_empty());
    }
    #[test]
    fn empty_first_frame_warmup_and_all_land_before_materialization() {
        let keys = keyboards();
        let row = prepare_row(&keys, 20.);
        let final_seeds = row.seeds.clone();
        let mut intro = Intro::default();
        intro.configure(&row, &keys, &[], &[]);
        assert_eq!(intro.stage(1_000), Stage::Waiting);
        assert!(!intro.start_if_ready(1_000, false));
        assert!(intro.start_if_ready(2_000, true));
        assert!(!intro.start_if_ready(3_000, true));
        assert_eq!(
            intro.stage(2_000),
            Stage::Flying {
                elapsed: 0,
                zoom: 0.
            }
        );
        for count in [1, 2, 32, 96] {
            for rank in 0..count {
                assert!(fly_offset(0, rank, count, 20.).is_none());
                assert_eq!(fly_offset(FLIGHT_MS, rank, count, 20.), Some(0.));
            }
        }
        assert_eq!(intro.stage(6_000), Stage::Revealing { amount: 0. });
        assert_eq!(intro.stage(7_500), Stage::Complete);
        intro.finish();
        intro.configure(&row, &keys, &[], &[]);
        assert_eq!(intro.stage(8_000), Stage::Complete);
        assert!(!intro.start_if_ready(8_000, true));
        assert_eq!(row.seeds, final_seeds);
    }
    #[test]
    fn roster_poll_during_entrance_preserves_the_clock_and_never_replays_afterwards() {
        let keys = keyboards();
        let mut intro = Intro::default();
        intro.configure(&prepare_row(&keys[..1], 20.), &keys[..1], &[], &[]);
        intro.start_if_ready(1_000, true);
        let before = intro.stage(4_000);
        intro.configure(&prepare_row(&keys, 20.), &keys, &[], &[]);
        assert_eq!(intro.stage(4_000), before);
        assert!(!intro.start_if_ready(4_000, true));
        assert_eq!(intro.stage(6_500), Stage::Complete);
        intro.finish();
        intro.configure(&prepare_row(&keys[..1], 20.), &keys[..1], &[], &[]);
        assert!(!intro.start_if_ready(10_000, true));
    }

    #[test]
    fn clipping_handles_lines_crossing_near_and_side_planes() {
        let camera = RetainedCamera {
            view_projection: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            ..RetainedCamera::default()
        };
        let line = clip_line(&camera, [-2., 0., -1.], [0., 0., 1.]).unwrap();
        assert_eq!(line, [[-1., 0., 0.], [0., 0., 1.]]);
        assert!(clip_line(&camera, [-3., 0., 0.5], [-2., 0., 0.5]).is_none());
        assert!(clip_line(&camera, [f32::NAN, 0., 0.], [0., 0., 1.]).is_none());
    }
    #[test]
    fn device_added_during_startup_reveal_keeps_its_own_slide_offset() {
        let keys = keyboards();
        let row = prepare_row(&keys, 20.);
        let mut intro = Intro::default();
        intro.configure(&row, &keys, &[], &[]);
        intro.start_if_ready(0, true);
        let camera = OrbitCamera::new(row.radius).retained(800, 600, [0.; 16]);
        let reveal = intro.stage(4_500);
        assert_eq!(intro.lines(reveal, &camera, [1.; 3], &[]).1.len(), 2);
        assert_eq!(
            intro.lines(reveal, &camera, [1.; 3], &[0., 1_000.]).1.len(),
            1
        );
    }

    #[test]
    fn outline_pass_is_empty_at_start_then_contains_only_clipped_line_pairs() {
        let keys = keyboards();
        let row = prepare_row(&keys, 20.);
        let mut intro = Intro::default();
        intro.configure(&row, &keys, &[], &[]);
        intro.start_if_ready(0, true);
        let camera = OrbitCamera::new(row.radius).retained(800, 600, [0.; 16]);
        assert!(
            intro
                .lines(intro.stage(0), &camera, [1.; 3], &[])
                .0
                .is_empty()
        );
        let (vertices, draws) = intro.lines(intro.stage(4_000), &camera, [1.; 3], &[]);
        assert_eq!(draws.len(), 2);
        assert_eq!(vertices.len() % 24, 0);
        assert_eq!(
            draws.iter().map(|d| d.count as usize).sum::<usize>(),
            vertices.len() / 12
        );
        for vertex in vertices.chunks_exact(12) {
            let p: [f32; 3] = core::array::from_fn(|i| {
                f32::from_le_bytes(vertex[i * 4..i * 4 + 4].try_into().unwrap())
            });
            assert!(p[0].abs() <= 1.001 && p[1].abs() <= 1.001 && p[2] >= -0.001 && p[2] <= 1.001);
        }
    }
}
