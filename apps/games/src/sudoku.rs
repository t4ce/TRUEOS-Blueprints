use sudokitty_core::{Difficulty, Game, Mask};

pub struct Sudoku {
    game: Game,
    pub cursor: (usize, usize),
}

impl Sudoku {
    pub fn new(seed: u64) -> Self {
        Self {
            game: Game::new(Difficulty::Easy, seed),
            cursor: (4, 4),
        }
    }

    pub fn reset(&mut self, seed: u64) {
        self.game = Game::new(Difficulty::Easy, seed);
        self.cursor = (4, 4);
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn select(&mut self, row: usize, col: usize) -> bool {
        if row >= 9 || col >= 9 {
            return false;
        }
        let changed = self.cursor != (col, row);
        self.cursor = (col, row);
        changed
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize) -> bool {
        let col = self.cursor.0.saturating_add_signed(dx).min(8);
        let row = self.cursor.1.saturating_add_signed(dy).min(8);
        self.select(row, col)
    }

    pub fn set_digit(&mut self, digit: u8) -> bool {
        let index = self.cursor.1 * 9 + self.cursor.0;
        self.game.set_digit(index, digit)
    }

    pub fn erase(&mut self) -> bool {
        let index = self.cursor.1 * 9 + self.cursor.0;
        self.game.erase(index)
    }

    pub fn violations(&self) -> Mask {
        self.game.violations()
    }
}
