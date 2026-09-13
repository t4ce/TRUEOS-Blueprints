//! An endless face-connected walk on the c1 shell around the center landmark.
use cubes_protocol::snake::{HALF, SEGMENTS, State, Step, on_shell};
pub struct Snake {
    pub state: State,
    direction: [i16; 3],
}
impl Snake {
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
                        0,
                    ]
                }),
            },
            direction: [1, 0, 0],
        }
    }
    pub fn step(&mut self, random: u32) -> Step {
        let head = self.state.cells[self.state.head as usize];
        let tail = (self.state.head as usize + 1) % SEGMENTS;
        let mut choices = [([0; 3], [0; 3]); 8];
        let mut count = 0;
        for axis in 0..3 {
            for sign in [-1, 1] {
                let mut direction = [0; 3];
                direction[axis] = sign;
                if direction == self.direction.map(|v| -v) {
                    continue;
                }
                let next = core::array::from_fn(|a| head[a] + direction[a]);
                if !on_shell(next)
                    || self
                        .state
                        .cells
                        .iter()
                        .enumerate()
                        .any(|(i, &p)| i != tail && p == next)
                {
                    continue;
                }
                // Favor straight travel, while allowing consecutive turns.
                let weight = if direction == self.direction { 3 } else { 1 };
                for _ in 0..weight {
                    choices[count] = (next, direction);
                    count += 1;
                }
            }
        }
        // On this bipartite shell, at most two neighbors are occupied by a
        // five-cell chain. Even its corners have three neighbors: no dead end.
        assert!(count > 0);
        let (cell, direction) = choices[random as usize % count];
        let step = Step {
            gallery: self.state.gallery,
            epoch: self.state.epoch,
            tick: self.state.tick.wrapping_add(1),
            slot: tail as u8,
            cell,
        };
        assert!(self.state.apply(step));
        self.direction = direction;
        step
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endless_walk_keeps_five_face_connected_cells_and_changes_only_the_tail() {
        let mut snake = Snake::new(7, 9);
        let mut random = 12345u32;
        let mut faces = [false; 6];
        let mut consecutive = false;
        let mut turned = false;
        for _ in 0..100_000 {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let old = snake.state;
            let dir = snake.direction;
            let step = snake.step(random);
            assert!(snake.state.valid());
            assert_eq!(State::parse(&snake.state.encode()), Some(snake.state));
            assert_eq!(Step::parse(&step.encode()), Some(step));
            for i in 0..SEGMENTS {
                if i != step.slot as usize {
                    assert_eq!(old.cells[i], snake.state.cells[i]);
                }
            }
            assert_eq!(
                (0..3).map(|a| dir[a] * snake.direction[a]).sum::<i16>(),
                if dir == snake.direction { 1 } else { 0 }
            );
            let turn = dir != snake.direction;
            consecutive |= turn && turned;
            turned = turn;
            for a in 0..3 {
                if step.cell[a] == HALF {
                    faces[a * 2] = true;
                }
                if step.cell[a] == -HALF - 1 {
                    faces[a * 2 + 1] = true;
                }
            }
        }
        assert!(faces.into_iter().all(|v| v));
        assert!(consecutive);
    }
    #[test]
    fn lost_duplicate_reordered_steps_recover_atomically_from_snapshot() {
        use cubes_protocol::snake::Replica;
        let mut snake = Snake::new(1, 2);
        let mut replica = Replica::default();
        assert!(replica.snapshot(snake.state));
        let first = snake.step(0);
        assert!(replica.step(first));
        assert!(!replica.step(first));
        snake.step(1);
        let third = snake.step(2);
        let old = replica.state;
        assert!(!replica.step(third));
        assert_eq!(replica.state, old);
        assert!(replica.needs_snapshot);
        assert!(replica.snapshot(snake.state));
        assert!(!replica.needs_snapshot);
        assert!(!replica.step(first));
        assert_eq!(replica.state, Some(snake.state));
        let old = snake.state;
        let missing = snake.step(0);
        let future = snake.step(0);
        assert!(!replica.step(future));
        assert!(replica.step(missing));
        assert!(replica.step(future));
        assert!(!replica.needs_snapshot);
        assert!(!replica.snapshot(old));
        let mut invalid = snake.state;
        invalid.cells[0] = [0, 0, 0];
        assert!(State::parse(&invalid.encode()).is_none());
        invalid = snake.state;
        invalid.cells[0] = invalid.cells[1];
        assert!(State::parse(&invalid.encode()).is_none());
        assert!(Step::parse(&future.encode()[..18]).is_none());
        snake.state.tick = u32::MAX;
        let mut wrapped = snake.state;
        assert!(wrapped.apply(snake.step(0)));
        assert_eq!(wrapped.tick, 0);
    }
}
