use alloc::{format, vec::Vec};
use cozy_chess::{Board, Color, GameStatus, Piece, Square, util::parse_uci_move};

pub struct Chess {
    board: Board,
    history: Vec<Board>,
    pub cursor: (usize, usize),
    pub selected: Option<Square>,
}

impl Chess {
    pub fn new() -> Self {
        let board = Board::default();
        Self {
            history: alloc::vec![board.clone()],
            board,
            cursor: (4, 6),
            selected: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn status(&self) -> GameStatus {
        let status = self.board.status();
        if status == GameStatus::Ongoing
            && self
                .history
                .iter()
                .filter(|past| self.board.same_position(past))
                .count()
                >= 3
        {
            GameStatus::Drawn
        } else {
            status
        }
    }

    pub fn select_cursor(&mut self, row: usize, col: usize) -> bool {
        if row >= 8 || col >= 8 {
            return false;
        }
        let changed = self.cursor != (col, row);
        self.cursor = (col, row);
        changed
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize) -> bool {
        let col = self.cursor.0.saturating_add_signed(dx).min(7);
        let row = self.cursor.1.saturating_add_signed(dy).min(7);
        self.select_cursor(row, col)
    }

    pub fn square(row: usize, col: usize) -> Square {
        Square::index((7 - row) * 8 + col)
    }

    fn candidate(&self, from: Square, to: Square) -> Option<cozy_chess::Move> {
        let promotion = self.board.piece_on(from) == Some(Piece::Pawn)
            && (to.rank() as usize == 0 || to.rank() as usize == 7);
        let uci = if promotion {
            format!("{from}{to}q")
        } else {
            format!("{from}{to}")
        };
        let mv = parse_uci_move(&self.board, &uci).ok()?;
        self.board.is_legal(mv).then_some(mv)
    }

    pub fn legal_at(&self, row: usize, col: usize) -> bool {
        let Some(from) = self.selected else {
            return false;
        };
        self.candidate(from, Self::square(row, col)).is_some()
    }

    pub fn activate(&mut self) -> bool {
        if self.status() != GameStatus::Ongoing {
            return false;
        }
        let to = Self::square(self.cursor.1, self.cursor.0);
        if let Some(from) = self.selected {
            if from == to {
                self.selected = None;
                return true;
            }
            if let Some(mv) = self.candidate(from, to) {
                if self.board.try_play(mv).is_ok() {
                    self.history.push(self.board.clone());
                    self.selected = None;
                    return true;
                }
            }
        }
        if self.board.color_on(to) == Some(self.board.side_to_move()) {
            let changed = self.selected != Some(to);
            self.selected = Some(to);
            changed
        } else {
            false
        }
    }

    pub fn side(&self) -> Color {
        self.board.side_to_move()
    }
}
