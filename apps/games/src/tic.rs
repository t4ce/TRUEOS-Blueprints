use gamie::tictactoe::{Game, Player, Status};
use trueos::input::TrueosKeyboardOutputEvent;
use trueos::ui4_scene::{CursorSource, InputRoute};

#[derive(Clone, Copy)]
pub struct Seat {
    pub cursor: CursorSource,
    pub combo_id: u32,
    pub color_rgba: u32,
    pub selected: bool,
    pub cell: (usize, usize),
}

pub struct TicTacToe {
    game: Game,
    seats: [Option<Seat>; 2],
}

impl TicTacToe {
    pub fn new() -> Self {
        Self {
            game: Game::new().unwrap(),
            seats: [None, None],
        }
    }

    pub fn reset(&mut self) {
        self.game = Game::new().unwrap();
        for seat in self.seats.iter_mut().flatten() {
            seat.cell = (1, 1);
        }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn seats(&self) -> &[Option<Seat>; 2] {
        &self.seats
    }

    pub fn sync_routes(&mut self, routes: &[InputRoute]) -> bool {
        let mut changed = false;
        for seat in self.seats.iter_mut().flatten() {
            let route = routes.iter().find(|route| route.cursor == seat.cursor);
            let selected = route.is_some_and(|route| route.selected_for_window);
            if seat.selected != selected {
                seat.selected = selected;
                changed = true;
            }
            if let Some(route) = route {
                seat.combo_id = route.combo_id;
                if seat.color_rgba != route.color_rgba {
                    seat.color_rgba = route.color_rgba;
                    changed = true;
                }
            }
        }
        for route in routes.iter().filter(|route| route.selected_for_window) {
            if self
                .seats
                .iter()
                .flatten()
                .any(|seat| seat.cursor == route.cursor)
            {
                continue;
            }
            if let Some(empty) = self
                .seats
                .iter_mut()
                .find(|seat| seat.is_none() || seat.is_some_and(|seat| !seat.selected))
            {
                *empty = Some(Seat {
                    cursor: route.cursor,
                    combo_id: route.combo_id,
                    color_rgba: route.color_rgba,
                    selected: true,
                    cell: (1, 1),
                });
                changed = true;
            }
        }
        changed
    }

    pub fn pointer_player(&self, source: CursorSource) -> Option<usize> {
        self.seats
            .iter()
            .position(|seat| seat.is_some_and(|seat| seat.selected && seat.cursor == source))
    }

    pub fn keyboard_player(
        &self,
        event: &TrueosKeyboardOutputEvent,
        routes: &[InputRoute],
    ) -> Option<usize> {
        self.seats.iter().enumerate().find_map(|(player, seat)| {
            let seat = seat.as_ref().filter(|seat| seat.selected)?;
            let route = routes
                .iter()
                .find(|route| route.selected_for_window && route.cursor == seat.cursor)?;
            let keyboard = route.keyboard?;
            (keyboard.controller_id == event.controller_id
                && keyboard.slot_id == event.slot_id
                && keyboard.ep_target == event.ep_target
                && (seat.combo_id == 0 || seat.combo_id == keyboard.combo_id))
                .then_some(player)
        })
    }

    pub fn select(&mut self, player: usize, row: usize, col: usize) -> bool {
        if row >= 3 || col >= 3 {
            return false;
        }
        let Some(seat) = self.seats.get_mut(player).and_then(Option::as_mut) else {
            return false;
        };
        let changed = seat.cell != (col, row);
        seat.cell = (col, row);
        changed
    }

    pub fn move_cursor(&mut self, player: usize, dx: isize, dy: isize) -> bool {
        let Some(seat) = self.seats.get_mut(player).and_then(Option::as_mut) else {
            return false;
        };
        let col = seat.cell.0.saturating_add_signed(dx).min(2);
        let row = seat.cell.1.saturating_add_signed(dy).min(2);
        self.select(player, row, col)
    }

    pub fn play_selected(&mut self, player: usize) -> bool {
        let Some(seat) = self.seats.get(player).and_then(Option::as_ref) else {
            return false;
        };
        self.play(player, seat.cell.1, seat.cell.0)
    }

    pub fn play(&mut self, player: usize, row: usize, col: usize) -> bool {
        if row >= 3
            || col >= 3
            || !self
                .seats
                .get(player)
                .is_some_and(|seat| seat.is_some_and(|seat| seat.selected))
        {
            return false;
        }
        let expected = if player == 0 {
            Player::Player0
        } else {
            Player::Player1
        };
        if !matches!(self.game.status(), Status::Ongoing) || self.game.next_player() != expected {
            return false;
        }
        self.game.put(row, col).is_ok()
    }
}
