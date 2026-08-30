//! Rules for the Shell2 Pi Snake Blueprint.
//!
//! The game model is deliberately independent from the Shell2 and HTTP
//! adapters so its collision and Pi-validation rules stay testable.

use std::collections::VecDeque;

pub const PORT: u16 = 45_329;
pub const PI: &str = "3.14159265358979323846264338327950288419716939937510";
pub const MAX_APPLES: usize = 4;
pub const MOVE_MS: u64 = 180;
pub const APPLE_MS: u64 = 1_000;
pub const CONTINUE_AFTER_PI_MS: u64 = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub const fn delta(self) -> (i16, i16) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    pub const fn opposite(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Up, Self::Down)
                | (Self::Down, Self::Up)
                | (Self::Left, Self::Right)
                | (Self::Right, Self::Left)
        )
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub joined: bool,
    pub snake: VecDeque<Cell>,
    pub direction: Direction,
    pub started: bool,
    pub awaiting_pi: bool,
    pub awaiting_direction: bool,
    pub pi_index: usize,
    pub pi_chart: bool,
    pub resume_at_ms: u64,
}

impl Player {
    fn new(joined: bool, direction: Direction) -> Self {
        Self {
            joined,
            snake: VecDeque::new(),
            direction,
            started: false,
            awaiting_pi: false,
            awaiting_direction: false,
            pi_index: 0,
            pi_chart: false,
            resume_at_ms: 0,
        }
    }

    fn reset_to_three(&mut self, cell: Cell) {
        self.snake.clear();
        self.snake.push_front(cell);
        self.started = true;
        self.awaiting_pi = false;
        self.awaiting_direction = false;
        self.pi_index = 1; // The visible head is the initial `3`; dot comes next.
        self.pi_chart = false;
    }

    pub fn expected_pi(&self) -> Option<char> {
        self.awaiting_pi
            .then(|| PI.chars().nth(self.pi_index))
            .flatten()
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    pub width: u16,
    pub height: u16,
    pub players: [Player; 2],
    pub apples: Vec<Cell>,
    pub status: String,
    next_move_ms: u64,
    next_apple_ms: u64,
    /// Latest game-loop time. Network requests use this rather than wall time
    /// so P2 receives the same three-second pause semantics as P1.
    pub clock_ms: u64,
    seed: u64,
    dirty: bool,
}

impl Game {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width: width.max(10),
            height: height.max(7),
            players: [
                Player::new(true, Direction::Right),
                Player::new(false, Direction::Left),
            ],
            apples: Vec::new(),
            status: "P1: press 3 to begin Pi Snake. P2 joins at :45329.".to_owned(),
            next_move_ms: MOVE_MS,
            next_apple_ms: APPLE_MS,
            clock_ms: 0,
            seed: 0x5049_534e_414b_4532,
            dirty: true,
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        let width = width.max(10);
        let height = height.max(7);
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        for player in &mut self.players {
            for cell in &mut player.snake {
                cell.x = cell.x.min(width - 1);
                cell.y = cell.y.min(height - 1);
            }
        }
        self.apples
            .retain(|apple| apple.x < width && apple.y < height);
        self.dirty = true;
    }

    pub fn join_remote(&mut self) -> bool {
        if self.players[1].joined {
            return false;
        }
        self.players[1].joined = true;
        self.status = "P2 joined. P2: press 3 in the browser to begin.".to_owned();
        self.dirty = true;
        true
    }

    pub fn input(&mut self, player_id: usize, key: char, now_ms: u64) {
        if player_id >= self.players.len() || !self.players[player_id].joined {
            return;
        }
        if key == '3' && !self.players[player_id].started {
            self.start(player_id);
            return;
        }
        if let Some(direction) = direction_for(key) {
            self.turn(player_id, direction);
            return;
        }
        self.answer_pi(player_id, key, now_ms);
    }

    pub fn turn(&mut self, player_id: usize, direction: Direction) {
        let player = &mut self.players[player_id];
        if !player.started || player.awaiting_pi || player.direction.opposite(direction) {
            return;
        }
        player.direction = direction;
        if player.awaiting_direction {
            player.awaiting_direction = false;
            self.status = format!("P{} continues.", player_id + 1);
        }
        self.dirty = true;
    }

    fn start(&mut self, player_id: usize) {
        if self.players[player_id].started {
            return;
        }
        let cell = self.spawn_for(player_id);
        self.players[player_id].reset_to_three(cell);
        self.status = format!(
            "P{} started: eat @, then type . to continue Pi.",
            player_id + 1
        );
        self.dirty = true;
    }

    fn answer_pi(&mut self, player_id: usize, key: char, now_ms: u64) {
        let player = &mut self.players[player_id];
        if !player.awaiting_pi {
            return;
        }
        let expected = player.expected_pi();
        if expected == Some(key) {
            player.awaiting_pi = false;
            player.awaiting_direction = true;
            player.resume_at_ms = now_ms.saturating_add(CONTINUE_AFTER_PI_MS);
            player.pi_index = player.pi_index.saturating_add(1);
            self.status = format!(
                "P{} correct ({key}). Arrow now, or it continues in 3 seconds.",
                player_id + 1
            );
        } else if player.pi_index == 1 {
            // Missing the first mandatory dot erases the one-cell `3` snake.
            // It becomes a literal Pi chart until the next apple restores it.
            player.snake.clear();
            player.pi_chart = true;
            player.awaiting_pi = false;
            player.awaiting_direction = false;
            self.status = format!(
                "P{} missed the mandatory dot: Pi chart mode. Next @ restores 3.",
                player_id + 1
            );
        } else {
            let _ = player.snake.pop_back();
            player.awaiting_pi = false;
            player.awaiting_direction = true;
            player.resume_at_ms = now_ms.saturating_add(CONTINUE_AFTER_PI_MS);
            self.status = format!(
                "P{} expected {:?}, lost one chain cell. Arrow now or auto-move in 3s.",
                player_id + 1,
                expected
            );
        }
        self.dirty = true;
    }

    pub fn update(&mut self, now_ms: u64) {
        self.clock_ms = now_ms;
        while now_ms >= self.next_apple_ms {
            if self.apples.len() < MAX_APPLES {
                self.spawn_apple();
            }
            self.next_apple_ms = self.next_apple_ms.saturating_add(APPLE_MS);
        }
        while now_ms >= self.next_move_ms {
            self.next_move_ms = self.next_move_ms.saturating_add(MOVE_MS);
            self.move_all(now_ms);
        }
    }

    fn move_all(&mut self, now_ms: u64) {
        for id in 0..self.players.len() {
            if !self.players[id].joined || !self.players[id].started {
                continue;
            }
            if self.players[id].awaiting_pi {
                continue;
            }
            if self.players[id].awaiting_direction {
                if now_ms < self.players[id].resume_at_ms {
                    continue;
                }
                self.players[id].awaiting_direction = false;
                self.status = format!("P{} auto-continued.", id + 1);
            }
            self.step_player(id, now_ms);
        }
    }

    fn step_player(&mut self, id: usize, now_ms: u64) {
        if self.players[id].pi_chart {
            // Pi-chart players do not travel. An already-spawned apple restores
            // their `3` head at a safe starting cell on the next game tick.
            if !self.apples.is_empty() {
                self.apples.pop();
                let cell = self.spawn_for(id);
                self.players[id].reset_to_three(cell);
                self.status = format!("P{}'s Pi chart found @ and became 3 again.", id + 1);
                self.dirty = true;
            }
            return;
        }
        let Some(head) = self.players[id].snake.front().copied() else {
            return;
        };
        let (dx, dy) = self.players[id].direction.delta();
        let x = head.x as i16 + dx;
        let y = head.y as i16 + dy;
        if x < 0 || y < 0 || x >= self.width as i16 || y >= self.height as i16 {
            self.respawn(id, "hit the wall");
            return;
        }
        let next = Cell {
            x: x as u16,
            y: y as u16,
        };
        if self.occupied_by_snake(next) {
            self.respawn(id, "cannot eat a snake");
            return;
        }
        let ate = self.apples.iter().position(|apple| *apple == next);
        self.players[id].snake.push_front(next);
        if let Some(index) = ate {
            self.apples.swap_remove(index);
            self.players[id].awaiting_pi = true;
            self.players[id].awaiting_direction = false;
            self.status = format!(
                "P{} ate @. Type the next Pi character: {:?}",
                id + 1,
                self.players[id].expected_pi()
            );
        } else {
            let _ = self.players[id].snake.pop_back();
        }
        let _ = now_ms;
        self.dirty = true;
    }

    fn respawn(&mut self, id: usize, reason: &str) {
        let cell = self.spawn_for(id);
        self.players[id].reset_to_three(cell);
        self.status = format!("P{} {reason}; reset to 3.", id + 1);
        self.dirty = true;
    }

    fn occupied_by_snake(&self, cell: Cell) -> bool {
        self.players
            .iter()
            .any(|player| player.snake.contains(&cell))
    }

    fn spawn_for(&self, id: usize) -> Cell {
        let x = if id == 0 {
            self.width / 4
        } else {
            self.width.saturating_mul(3) / 4
        };
        let y = self.height / 2;
        Cell { x, y }
    }

    fn spawn_apple(&mut self) {
        for _ in 0..self.width as usize * self.height as usize {
            let x = (self.next_random() % u64::from(self.width)) as u16;
            let y = (self.next_random() % u64::from(self.height)) as u16;
            let cell = Cell { x, y };
            if !self.occupied_by_snake(cell) && !self.apples.contains(&cell) {
                self.apples.push(cell);
                self.dirty = true;
                return;
            }
        }
    }

    fn next_random(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.seed
    }
}

pub fn direction_for(key: char) -> Option<Direction> {
    match key.to_ascii_lowercase() {
        'w' => Some(Direction::Up),
        'a' => Some(Direction::Left),
        's' => Some(Direction::Down),
        'd' => Some(Direction::Right),
        _ => None,
    }
}

/// Return the Pi character for a segment indexed from the snake's head.
///
/// Positions are stored head-first, whereas the visible number reads from tail
/// to head: a right-moving six-cell snake is therefore `3.1415`.
pub fn snake_glyph(head_index: usize, snake_length: usize) -> char {
    PI.chars()
        .nth(snake_length.saturating_sub(head_index.saturating_add(1)))
        .unwrap_or('π')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_three_then_requires_dot() {
        let mut game = Game::new(20, 10);
        game.input(0, '3', 0);
        game.players[0].awaiting_pi = true;
        assert_eq!(game.players[0].expected_pi(), Some('.'));
        game.input(0, '.', 5);
        assert_eq!(game.players[0].pi_index, 2);
        assert!(game.players[0].awaiting_direction);
    }

    #[test]
    fn missing_dot_becomes_pi_chart() {
        let mut game = Game::new(20, 10);
        game.input(0, '3', 0);
        game.players[0].awaiting_pi = true;
        game.input(0, 'x', 5);
        assert!(game.players[0].pi_chart);
        assert!(game.players[0].snake.is_empty());
    }

    #[test]
    fn a_snake_cannot_turn_back_into_itself() {
        let mut game = Game::new(20, 10);
        game.input(0, '3', 0);
        game.turn(0, Direction::Left);
        assert_eq!(game.players[0].direction, Direction::Right);
    }

    #[test]
    fn snake_segments_read_as_pi_from_tail_to_head() {
        let displayed: String = (0..6)
            .rev()
            .map(|head_index| snake_glyph(head_index, 6))
            .collect();
        assert_eq!(displayed, "3.1415");
    }
}
