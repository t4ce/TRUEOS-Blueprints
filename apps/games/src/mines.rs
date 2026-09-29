use alloc::vec::Vec;
use gamie::minesweeper::{Cell, CellStatus, Game, Status};
use rand::{SeedableRng, rngs::SmallRng};

pub const COLS: usize = 10;
pub const ROWS: usize = 20;
pub const MINES: usize = 30;

pub struct Minefield {
    game: Game,
    seed: u64,
    first_reveal: bool,
    pub cursor: (usize, usize),
}

impl Minefield {
    pub fn new(seed: u64) -> Self {
        Self {
            game: make_game(seed),
            seed,
            first_reveal: true,
            cursor: (COLS / 2, ROWS / 2),
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        *self.game.get(row, col)
    }

    pub fn status(&self) -> &Status {
        self.game.status()
    }

    pub fn flag_count(&self) -> usize {
        self.game.flag_count()
    }

    pub fn revealed_count(&self) -> usize {
        (0..ROWS)
            .flat_map(|row| (0..COLS).map(move |col| (row, col)))
            .filter(|&(row, col)| matches!(self.game.get(row, col).status(), CellStatus::Revealed))
            .count()
    }

    pub fn select(&mut self, row: usize, col: usize) {
        if row < ROWS && col < COLS {
            self.cursor = (col, row);
        }
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize) {
        self.cursor.0 = self.cursor.0.saturating_add_signed(dx).min(COLS - 1);
        self.cursor.1 = self.cursor.1.saturating_add_signed(dy).min(ROWS - 1);
    }

    pub fn reveal(&mut self) -> bool {
        let (col, row) = self.cursor;
        if self.first_reveal && matches!(self.game.get(row, col).status(), CellStatus::Hidden) {
            // Gamie places mines at construction. Reroll only an unlucky first
            // reveal, then restore flags already placed by the player.
            if self.game.get(row, col).is_mine() {
                let flags: Vec<_> = (0..ROWS)
                    .flat_map(|r| (0..COLS).map(move |c| (r, c)))
                    .filter(|&(r, c)| matches!(self.game.get(r, c).status(), CellStatus::Flagged))
                    .collect();
                loop {
                    self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
                    let next = make_game(self.seed);
                    if !next.get(row, col).is_mine() {
                        self.game = next;
                        for &(r, c) in &flags {
                            let _ = self.game.flag(r, c);
                        }
                        break;
                    }
                }
            }
        }
        if self.game.click(row, col, true).is_ok() {
            self.first_reveal = false;
            true
        } else {
            false
        }
    }

    pub fn flag(&mut self) -> bool {
        let (col, row) = self.cursor;
        self.game.flag(row, col).is_ok()
    }
}

fn make_game(seed: u64) -> Game {
    let mut rng = SmallRng::seed_from_u64(seed);
    Game::new(&mut rng, COLS, ROWS, MINES).expect("fixed minefield dimensions are valid")
}
