#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Ideas,
    Doing,
    Done,
}

impl Lane {
    pub const ALL: [Self; 3] = [Self::Ideas, Self::Doing, Self::Done];

    pub const fn index(self) -> usize {
        match self {
            Self::Ideas => 0,
            Self::Doing => 1,
            Self::Done => 2,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Ideas => "IDEAS",
            Self::Doing => "DOING",
            Self::Done => "DONE",
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Ideas => Self::Ideas,
            Self::Doing => Self::Ideas,
            Self::Done => Self::Doing,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Ideas => Self::Doing,
            Self::Doing => Self::Done,
            Self::Done => Self::Done,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Card {
    pub id: u64,
    pub title: String,
    pub detail: String,
    pub lane: Lane,
}

#[derive(Clone, Debug)]
pub struct Board {
    cards: Vec<Card>,
    next_id: u64,
}

impl Board {
    pub fn new(seed_demo: bool) -> Self {
        let mut board = Self {
            cards: Vec::new(),
            next_id: 1,
        };
        if seed_demo {
            board.seed_demo();
        }
        board
    }

    pub fn reset(&mut self, seed_demo: bool) {
        self.cards.clear();
        self.next_id = 1;
        if seed_demo {
            self.seed_demo();
        }
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn cards_in(&self, lane: Lane) -> Vec<&Card> {
        self.cards.iter().filter(|card| card.lane == lane).collect()
    }

    pub fn first_id(&self, lane: Lane) -> Option<u64> {
        self.cards.iter().find(|card| card.lane == lane).map(|card| card.id)
    }

    pub fn get(&self, id: u64) -> Option<&Card> {
        self.cards.iter().find(|card| card.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Card> {
        self.cards.iter_mut().find(|card| card.id == id)
    }

    pub fn add(&mut self, lane: Lane, title: String) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.cards.push(Card {
            id,
            title,
            detail: String::new(),
            lane,
        });
        id
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.cards.len();
        self.cards.retain(|card| card.id != id);
        before != self.cards.len()
    }

    pub fn move_to(&mut self, id: u64, lane: Lane) -> bool {
        let Some(card) = self.get_mut(id) else {
            return false;
        };
        if card.lane == lane {
            return false;
        }
        card.lane = lane;
        true
    }

    fn seed_demo(&mut self) {
        let ideas = [
            ("Braille paint app", "mouse drawing with undo"),
            ("Package graph", "show dependencies as a living map"),
            ("Boot tune", "make the startup sound shamelessly good"),
        ];
        let doing = [
            ("redb RAM explorer", "runs end to end on TrueOS"),
            ("terminal polish", "retained frame + tiny changed runs"),
        ];
        let done = [
            ("custom Rust OS", "apparently this part was the easy bit"),
            ("Texplo", "the surface all other apps learn from"),
        ];
        for (title, detail) in ideas {
            let id = self.add(Lane::Ideas, title.to_owned());
            self.get_mut(id).unwrap().detail = detail.to_owned();
        }
        for (title, detail) in doing {
            let id = self.add(Lane::Doing, title.to_owned());
            self.get_mut(id).unwrap().detail = detail.to_owned();
        }
        for (title, detail) in done {
            let id = self.add(Lane::Done, title.to_owned());
            self.get_mut(id).unwrap().detail = detail.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Board, Lane};

    #[test]
    fn card_lifecycle_stays_in_one_owned_model() {
        let mut board = Board::new(false);
        let id = board.add(Lane::Ideas, "ship it".to_owned());
        assert_eq!(board.cards().len(), 1);
        assert_eq!(board.get(id).unwrap().lane, Lane::Ideas);

        assert!(board.move_to(id, Lane::Doing));
        assert_eq!(board.get(id).unwrap().lane, Lane::Doing);
        assert!(!board.move_to(id, Lane::Doing));

        assert!(board.delete(id));
        assert!(board.cards().is_empty());
        assert!(!board.delete(id));
    }

    #[test]
    fn reset_can_switch_between_empty_and_demo_sessions() {
        let mut board = Board::new(true);
        assert!(!board.cards().is_empty());
        board.reset(false);
        assert!(board.cards().is_empty());
        board.reset(true);
        assert!(!board.cards().is_empty());
    }
}
