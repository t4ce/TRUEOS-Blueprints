#![no_std]
//! Subtractive Tetris rules.
//!
//! Full rows enter at the top and push the field toward the bottom. The active
//! tetromino is a cutter: at its selected horizontal position, it removes the
//! lowest intact copy of its four-cell mask. Existing holes cannot be cut a
//! second time, but surrounding material is deliberately allowed.

use core::cmp::{max, min};

pub const PIECE_CELL_COUNT: usize = 4;
pub const START_ROW_INTERVAL_MS: u32 = 4_000;
pub const MIN_ROW_INTERVAL_MS: u32 = 1_800;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    pub const ALL: [Self; 7] = [
        Self::I,
        Self::O,
        Self::T,
        Self::S,
        Self::Z,
        Self::J,
        Self::L,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Cw,
    Ccw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cutter {
    pub kind: PieceKind,
    pub rotation: u8,
    pub x: usize,
}

impl Cutter {
    const fn new(kind: PieceKind, x: usize) -> Self {
        Self {
            kind,
            rotation: 0,
            x,
        }
    }

    pub fn cells(self) -> [(u8, u8); PIECE_CELL_COUNT] {
        piece_cells(self.kind, self.rotation)
    }

    pub fn dimensions(self) -> (usize, usize) {
        piece_dimensions(self.kind, self.rotation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CutResult {
    Cut,
    NoFit,
    GameOver,
}

#[derive(Clone, Copy, Debug)]
struct Lcg32(u32);

impl Lcg32 {
    const fn new(seed: u32) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

#[derive(Clone, Debug)]
struct SevenBag {
    pieces: [PieceKind; 7],
    remaining: usize,
}

impl SevenBag {
    const fn new() -> Self {
        Self {
            pieces: PieceKind::ALL,
            remaining: 0,
        }
    }

    fn draw(&mut self, rng: &mut Lcg32) -> PieceKind {
        if self.remaining == 0 {
            self.pieces = PieceKind::ALL;
            for index in (1..self.pieces.len()).rev() {
                let swap = rng.next_u32() as usize % (index + 1);
                self.pieces.swap(index, swap);
            }
            self.remaining = self.pieces.len();
        }
        self.remaining -= 1;
        self.pieces[self.remaining]
    }
}

#[derive(Clone, Debug)]
pub struct Game<const W: usize, const H: usize> {
    board: [[Option<u8>; W]; H],
    cutter: Cutter,
    next: PieceKind,
    rng: Lcg32,
    bag: SevenBag,
    row_elapsed_ms: u32,
    rows_spawned: u32,
    cuts: u32,
    invalid_cuts: u32,
    score: u32,
    game_over: bool,
    changed: bool,
}

impl<const W: usize, const H: usize> Game<W, H> {
    pub fn new(seed: u32) -> Self {
        assert!(W >= 4 && H >= 4, "Cut Tetris needs at least a 4x4 field");
        let mut rng = Lcg32::new(seed);
        let mut bag = SevenBag::new();
        let first = bag.draw(&mut rng);
        let next = bag.draw(&mut rng);
        let mut game = Self {
            board: [[None; W]; H],
            cutter: Cutter::new(first, centered_x::<W>(first, 0)),
            next,
            rng,
            bag,
            row_elapsed_ms: 0,
            rows_spawned: 0,
            cuts: 0,
            invalid_cuts: 0,
            score: 0,
            game_over: false,
            changed: true,
        };
        // One immediate row lets a horizontal cutter start carving instead of
        // presenting an empty board for the whole first interval.
        game.spawn_row();
        game
    }

    pub const fn width(&self) -> usize {
        W
    }

    pub const fn height(&self) -> usize {
        H
    }

    pub const fn cutter(&self) -> Cutter {
        self.cutter
    }

    pub const fn next_piece(&self) -> PieceKind {
        self.next
    }

    pub const fn rows_spawned(&self) -> u32 {
        self.rows_spawned
    }

    pub const fn cuts(&self) -> u32 {
        self.cuts
    }

    pub const fn invalid_cuts(&self) -> u32 {
        self.invalid_cuts
    }

    pub const fn score(&self) -> u32 {
        self.score
    }

    pub const fn is_game_over(&self) -> bool {
        self.game_over
    }

    pub const fn row_elapsed_ms(&self) -> u32 {
        self.row_elapsed_ms
    }

    pub fn row_interval_ms(&self) -> u32 {
        let speed_steps = self.rows_spawned / 6;
        max(
            MIN_ROW_INTERVAL_MS,
            START_ROW_INTERVAL_MS.saturating_sub(speed_steps.saturating_mul(175)),
        )
    }

    pub fn cell_at(&self, x: usize, y: usize) -> Option<u8> {
        if x >= W || y >= H {
            return None;
        }
        self.board[y][x]
    }

    pub fn filled_cell_count(&self) -> usize {
        self.board
            .iter()
            .flat_map(|row| row.iter())
            .filter(|cell| cell.is_some())
            .count()
    }

    pub const fn has_changed(&self) -> bool {
        self.changed
    }

    pub fn consume_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn target_cells(&self) -> Option<[(usize, usize); PIECE_CELL_COUNT]> {
        if self.game_over {
            return None;
        }
        self.target_cells_for(self.cutter)
    }

    pub fn move_left(&mut self) -> bool {
        if self.game_over || self.cutter.x == 0 {
            return false;
        }
        self.cutter.x -= 1;
        self.changed = true;
        true
    }

    pub fn move_right(&mut self) -> bool {
        let (width, _) = self.cutter.dimensions();
        if self.game_over || self.cutter.x + width >= W {
            return false;
        }
        self.cutter.x += 1;
        self.changed = true;
        true
    }

    pub fn rotate(&mut self, direction: Rotation) -> bool {
        if self.game_over {
            return false;
        }
        self.cutter.rotation = match direction {
            Rotation::Cw => (self.cutter.rotation + 1) & 3,
            Rotation::Ccw => (self.cutter.rotation + 3) & 3,
        };
        let (width, _) = self.cutter.dimensions();
        self.cutter.x = min(self.cutter.x, W - width);
        self.changed = true;
        true
    }

    pub fn cut(&mut self) -> CutResult {
        if self.game_over {
            return CutResult::GameOver;
        }
        let Some(target) = self.target_cells() else {
            self.invalid_cuts = self.invalid_cuts.saturating_add(1);
            self.changed = true;
            return CutResult::NoFit;
        };

        let mut deepest = 0_usize;
        for (x, y) in target {
            deepest = max(deepest, y);
            self.board[y][x] = None;
        }
        self.cuts = self.cuts.saturating_add(1);
        self.score = self
            .score
            .saturating_add(100 + (deepest as u32).saturating_mul(5));

        self.cutter = Cutter::new(self.next, centered_x::<W>(self.next, 0));
        self.next = self.bag.draw(&mut self.rng);
        self.changed = true;
        CutResult::Cut
    }

    pub fn tick(&mut self, elapsed_ms: u32) {
        if self.game_over {
            return;
        }
        self.row_elapsed_ms = self.row_elapsed_ms.saturating_add(elapsed_ms);
        loop {
            let interval = self.row_interval_ms();
            if self.row_elapsed_ms < interval {
                break;
            }
            self.row_elapsed_ms -= interval;
            self.spawn_row();
            if self.game_over {
                break;
            }
        }
    }

    fn spawn_row(&mut self) {
        if self.board[H - 1].iter().any(Option::is_some) {
            self.game_over = true;
            self.changed = true;
            return;
        }

        for y in (1..H).rev() {
            self.board[y] = self.board[y - 1];
        }
        let band = (self.rows_spawned % 6) as u8;
        self.board[0] = [Some(band); W];
        self.rows_spawned = self.rows_spawned.saturating_add(1);
        self.changed = true;
    }

    fn target_cells_for(&self, cutter: Cutter) -> Option<[(usize, usize); PIECE_CELL_COUNT]> {
        let cells = cutter.cells();
        let (width, height) = cutter.dimensions();
        if cutter.x + width > W || height > H {
            return None;
        }

        // The cutter has no vertical control. It stamps the lowest intact copy
        // of its mask at the selected x, which keeps the pressure near danger
        // while allowing surrounding blocks to remain and form real holes.
        for anchor_y in (0..=H - height).rev() {
            let mut target = [(0_usize, 0_usize); PIECE_CELL_COUNT];
            let mut intact = true;
            for (index, (cell_x, cell_y)) in cells.into_iter().enumerate() {
                let x = cutter.x + cell_x as usize;
                let y = anchor_y + cell_y as usize;
                target[index] = (x, y);
                if self.board[y][x].is_none() {
                    intact = false;
                    break;
                }
            }
            if intact {
                return Some(target);
            }
        }
        None
    }
}

fn centered_x<const W: usize>(kind: PieceKind, rotation: u8) -> usize {
    let (width, _) = piece_dimensions(kind, rotation);
    W.saturating_sub(width) / 2
}

pub fn piece_dimensions(kind: PieceKind, rotation: u8) -> (usize, usize) {
    let cells = piece_cells(kind, rotation);
    let mut width = 0_usize;
    let mut height = 0_usize;
    for (x, y) in cells {
        width = max(width, x as usize + 1);
        height = max(height, y as usize + 1);
    }
    (width, height)
}

pub fn piece_cells(kind: PieceKind, rotation: u8) -> [(u8, u8); PIECE_CELL_COUNT] {
    let mut cells = match kind {
        PieceKind::I => [(0, 0), (0, 1), (0, 2), (0, 3)],
        PieceKind::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
        PieceKind::T => [(1, 0), (0, 1), (1, 1), (2, 1)],
        PieceKind::S => [(1, 0), (2, 0), (0, 1), (1, 1)],
        PieceKind::Z => [(0, 0), (1, 0), (1, 1), (2, 1)],
        PieceKind::J => [(0, 0), (0, 1), (0, 2), (1, 2)],
        PieceKind::L => [(1, 0), (1, 1), (0, 2), (1, 2)],
    };

    if matches!(kind, PieceKind::O) {
        return cells;
    }

    for _ in 0..(rotation & 3) {
        for (x, y) in &mut cells {
            let old_x = *x;
            *x = 3 - *y;
            *y = old_x;
        }
        normalize(&mut cells);
    }
    cells
}

fn normalize(cells: &mut [(u8, u8); PIECE_CELL_COUNT]) {
    let mut min_x = u8::MAX;
    let mut min_y = u8::MAX;
    for (x, y) in cells.iter().copied() {
        min_x = min(min_x, x);
        min_y = min(min_y, y);
    }
    for (x, y) in cells {
        *x -= min_x;
        *y -= min_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestGame = Game<10, 20>;

    fn set_cutter(game: &mut TestGame, kind: PieceKind, rotation: u8, x: usize) {
        game.cutter = Cutter { kind, rotation, x };
    }

    #[test]
    fn horizontal_i_can_cut_the_first_row() {
        let mut game = TestGame::new(1);
        set_cutter(&mut game, PieceKind::I, 1, 0);

        assert_eq!(game.target_cells().map(|cells| cells[0].1), Some(0));
        assert_eq!(game.cut(), CutResult::Cut);
        assert_eq!(game.filled_cell_count(), 6);
    }

    #[test]
    fn vertical_i_waits_for_four_rows() {
        let mut game = TestGame::new(2);
        set_cutter(&mut game, PieceKind::I, 0, 0);
        assert!(game.target_cells().is_none());

        game.spawn_row();
        game.spawn_row();
        assert!(game.target_cells().is_none());

        game.spawn_row();
        assert!(game.target_cells().is_some());
    }

    #[test]
    fn existing_holes_are_not_cut_twice() {
        let mut game = TestGame::new(3);
        game.spawn_row();
        set_cutter(&mut game, PieceKind::I, 1, 0);
        assert_eq!(game.cut(), CutResult::Cut);

        set_cutter(&mut game, PieceKind::I, 1, 0);
        let target = game
            .target_cells()
            .expect("the intact row above still fits");
        assert!(target.into_iter().all(|(_, y)| y == 0));
    }

    #[test]
    fn an_invalid_cut_keeps_the_current_piece() {
        let mut game = TestGame::new(4);
        set_cutter(&mut game, PieceKind::O, 0, 0);
        let before = game.cutter();

        assert_eq!(game.cut(), CutResult::NoFit);
        assert_eq!(game.cutter(), before);
        assert_eq!(game.invalid_cuts(), 1);
    }

    #[test]
    fn a_row_spawn_pushes_holes_toward_the_bottom() {
        let mut game = TestGame::new(5);
        set_cutter(&mut game, PieceKind::I, 1, 0);
        assert_eq!(game.cut(), CutResult::Cut);
        assert!(game.cell_at(0, 0).is_none());

        game.spawn_row();
        assert!(game.cell_at(0, 0).is_some());
        assert!(game.cell_at(0, 1).is_none());
    }

    #[test]
    fn pushing_any_block_past_the_bottom_is_game_over() {
        let mut game = TestGame::new(6);
        game.board[TestGame::new(0).height() - 1][7] = Some(0);

        game.spawn_row();

        assert!(game.is_game_over());
    }

    #[test]
    fn seven_bag_contains_every_standard_piece_once() {
        let mut rng = Lcg32::new(7);
        let mut bag = SevenBag::new();
        let mut seen = [false; 7];
        for _ in 0..7 {
            seen[bag.draw(&mut rng) as usize] = true;
        }
        assert!(seen.into_iter().all(|present| present));
    }
}
