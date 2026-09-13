//! Straight surface travel with occasional immediate tunnels to the far face.
use cubes_protocol::worm::{HALF, SEGMENTS, State, Step, tunnel};
pub struct Worm {
    pub state: State,
    direction: [i16; 3],
    normal: [i16; 3],
}
impl Worm {
    pub fn new(gallery: u32, epoch: u32) -> Self {
        Self {
            state: State {
                gallery,
                epoch,
                tick: 0,
                head: 0,
                cells: core::array::from_fn(|i| {
                    [
                        if i == 0 {
                            0
                        } else {
                            i as i16 - SEGMENTS as i16
                        },
                        HALF,
                        3,
                    ]
                }),
            },
            direction: [1, 0, 0],
            normal: [0, 1, 0],
        }
    }
    pub fn step(&mut self, random: u32) -> Step {
        let head = self.state.cells[self.state.head as usize];
        let normal_axis = self.normal.iter().position(|&v| v != 0).unwrap();
        let mut opposite = head;
        opposite[normal_axis] = -1 - head[normal_axis];
        let cell = if random % 16 == 0 && tunnel(head, opposite) {
            // No side turn: retain the tangent heading after emerging.
            self.normal = self.normal.map(|v| -v);
            opposite
        } else {
            let axis = self.direction.iter().position(|&v| v != 0).unwrap();
            if head[axis] + self.direction[axis] > HALF
                || head[axis] + self.direction[axis] < -HALF - 1
            {
                // The edge connector is reached before folding, so a normal
                // surface step always shares a full face with its predecessor.
                let old_normal = self.normal;
                self.normal = self.direction;
                self.direction = old_normal.map(|v| -v);
            }
            core::array::from_fn(|a| head[a] + self.direction[a])
        };
        let step = Step {
            gallery: self.state.gallery,
            epoch: self.state.epoch,
            tick: self.state.tick.wrapping_add(1),
            slot: (self.state.head + 1) % SEGMENTS as u8,
            cell,
        };
        // Stream validation permits overlap; there is no body or snake lookup.
        assert!(self.state.apply(step));
        step
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn straight_travel_only_folds_at_edges_and_surviving_slots_do_not_change() {
        let mut worm = Worm::new(1, 2);
        for _ in 0..1000 {
            let old = worm.state;
            let direction = worm.direction;
            let axis = direction.iter().position(|&v| v != 0).unwrap();
            let next = old.cells[old.head as usize][axis] + direction[axis];
            let step = worm.step(1); // Never tunnel.
            if (-HALF - 1..=HALF).contains(&next) {
                assert_eq!(worm.direction, direction);
            }
            assert!(cubes_protocol::snake::adjacent(
                old.cells[old.head as usize],
                step.cell
            ));
            assert!(worm.state.valid());
            for i in 0..SEGMENTS {
                if i != step.slot as usize {
                    assert_eq!(old.cells[i], worm.state.cells[i]);
                }
            }
        }
    }
    #[test]
    fn tunnels_keep_heading_and_allow_self_overlap_without_collision() {
        let mut worm = Worm::new(1, 2);
        let start = worm.state;
        let step = worm.step(0);
        assert_eq!(step.cell, [0, -HALF - 1, 3]);
        assert_eq!(worm.direction, [1, 0, 0]);
        let step = worm.step(0);
        assert_eq!(step.cell, start.cells[0]); // Deliberate overlap with its body.
        assert!(worm.state.valid());
        assert_eq!(State::parse(&worm.state.encode()), Some(worm.state));
    }
    #[test]
    fn wire_rejects_off_shell_and_non_opposite_jumps() {
        let mut worm = Worm::new(1, 2);
        let mut step = worm.step(0);
        let mut state = Worm::new(1, 2).state;
        step.cell[2] += 1;
        let before = state;
        assert!(!state.apply(step));
        assert_eq!(state, before);
        step.cell = [0; 3];
        assert!(Step::parse(&step.encode()).is_none());
        let mut bytes = before.encode();
        bytes[12] = SEGMENTS as u8;
        assert!(State::parse(&bytes).is_none());
        assert!(cubes_protocol::snake::State::parse(&before.encode()).is_none());
    }
    #[test]
    fn endless_mixed_walk_and_packet_loss_recover_independently() {
        let mut worm = Worm::new(1, 2);
        let mut replica = cubes_protocol::worm::Replica::default();
        replica.snapshot(worm.state);
        let mut random = 12345u32;
        let mut tunnels = 0;
        for i in 0..100_000 {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let old = worm.state;
            let step = worm.step(random);
            tunnels += usize::from(tunnel(old.cells[old.head as usize], step.cell));
            assert_eq!(Step::parse(&step.encode()), Some(step));
            assert!(worm.state.valid());
            if i % 17 == 0 {
                continue;
            }
            if !replica.step(step) {
                assert!(replica.needs_snapshot);
                replica.snapshot(worm.state);
            }
            assert_eq!(replica.state, Some(worm.state));
        }
        assert!(tunnels > 1000);
    }
}
