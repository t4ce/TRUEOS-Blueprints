//! Games selector assembled from Cubes' compact Cube UI dialog raster model.
use alloc::vec::Vec;
use trueos::ui4_scene::{Damage, Error, Frame, SpriteCorner, SpriteQuad, rgba};

use crate::{Mode, dialog};

const SPRITE_ID: u32 = 2;
const DEFINITIONS: [dialog::Definition; 12] = [
    dialog::Definition {
        title: "TETRIS",
        text: "FALLING BLOCKS",
        button: "LAUNCH",
        theme: 1,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "MINESWEEPER",
        text: "CLEAR THE FIELD",
        button: "LAUNCH",
        theme: 0,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "TIC TAC TOE",
        text: "TWO PLAYER TURNS",
        button: "LAUNCH",
        theme: 2,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "SUDOKU",
        text: "NINE BY NINE",
        button: "LAUNCH",
        theme: 1,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "CHESS",
        text: "THE CUBE BOARD",
        button: "LAUNCH",
        theme: 0,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "CUT TETRIS",
        text: "CARVE THE BLOCKS",
        button: "LAUNCH",
        theme: 2,
        checkbox: true,
        enabled: true,
    },
    dialog::Definition {
        title: "GAME 07",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 1,
        checkbox: false,
        enabled: false,
    },
    dialog::Definition {
        title: "GAME 08",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 0,
        checkbox: false,
        enabled: false,
    },
    dialog::Definition {
        title: "GAME 09",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 2,
        checkbox: false,
        enabled: false,
    },
    dialog::Definition {
        title: "GAME 10",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 1,
        checkbox: false,
        enabled: false,
    },
    dialog::Definition {
        title: "GAME 11",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 0,
        checkbox: false,
        enabled: false,
    },
    dialog::Definition {
        title: "GAME 12",
        text: "COMING SOON",
        button: "LOCKED",
        theme: 2,
        checkbox: false,
        enabled: false,
    },
];
const MODES: [Mode; 6] = [
    Mode::Tetris,
    Mode::Minesweeper,
    Mode::TicTacToe,
    Mode::Sudoku,
    Mode::Chess,
    Mode::CutTetris,
];

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

pub struct Gallery {
    pub open: bool,
    pub selected: usize,
    pub fresh: [bool; 6],
    hover: Option<(usize, dialog::Widget)>,
    revision: u64,
    uploaded: u64,
}

impl Gallery {
    pub const fn new() -> Self {
        Self {
            open: false,
            selected: 0,
            fresh: [false; 6],
            hover: None,
            revision: 1,
            uploaded: 0,
        }
    }

    fn columns(width: u32, height: u32) -> usize {
        if width as f32 / height.max(1) as f32 >= 1.2 {
            4
        } else {
            3
        }
    }

    fn card(index: usize, width: u32, height: u32) -> Rect {
        let columns = Self::columns(width, height);
        let rows = 12 / columns;
        let margin = (width.min(height) as f32 * 0.025).max(12.0);
        let gap = margin * 0.55;
        let w = (width as f32 - 2.0 * margin - (columns - 1) as f32 * gap) / columns as f32;
        let h = (height as f32 - 2.0 * margin - (rows - 1) as f32 * gap) / rows as f32;
        Rect {
            x: margin + (index % columns) as f32 * (w + gap),
            y: margin + (index / columns) as f32 * (h + gap),
            w,
            h,
        }
    }

    fn panel(index: usize, width: u32, height: u32) -> Rect {
        let card = Self::card(index, width, height);
        let scale = (card.w / dialog::WIDTH as f32).min(card.h / dialog::HEIGHT as f32) * 0.94;
        let w = dialog::WIDTH as f32 * scale;
        let h = dialog::HEIGHT as f32 * scale;
        Rect {
            x: card.x + (card.w - w) * 0.5,
            y: card.y + (card.h - h) * 0.5,
            w,
            h,
        }
    }

    pub fn pointer(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        pressed: bool,
    ) -> (bool, Option<Mode>) {
        let panel = (0..12).find_map(|i| {
            let rect = Self::panel(i, width, height);
            rect.contains(x as f32, y as f32).then_some((i, rect))
        });
        let hover = panel.and_then(|(i, rect)| {
            let px = (x as f32 - rect.x) * dialog::WIDTH as f32 / rect.w;
            let py = (y as f32 - rect.y) * dialog::HEIGHT as f32 / rect.h;
            dialog::hit(&DEFINITIONS[i], px, py).map(|widget| (i, widget))
        });
        let mut changed = self.hover != hover;
        if changed {
            self.hover = hover;
            self.revision += 1;
        }
        if !pressed {
            return (changed, None);
        }
        let Some((index, _)) = panel else {
            return (changed, None);
        };
        if self.selected != index {
            self.selected = index;
            self.revision += 1;
            changed = true;
        }
        match hover {
            Some((_, dialog::Widget::Checkbox)) => {
                self.toggle_fresh();
                (true, None)
            }
            Some((_, dialog::Widget::Button)) => (changed, MODES.get(index).copied()),
            None => (changed, None),
        }
    }

    pub fn toggle_fresh(&mut self) {
        if self.selected < self.fresh.len() {
            self.fresh[self.selected] = !self.fresh[self.selected];
            self.revision += 1;
        }
    }

    pub fn move_selection(&mut self, key: u16, width: u32, height: u32) -> bool {
        use trueos::input::*;
        let columns = Self::columns(width, height);
        let index = self.selected;
        self.selected = match key {
            KEYBOARD_KEY_ARROW_LEFT if index % columns > 0 => index - 1,
            KEYBOARD_KEY_ARROW_RIGHT if index % columns + 1 < columns => index + 1,
            KEYBOARD_KEY_ARROW_UP if index >= columns => index - columns,
            KEYBOARD_KEY_ARROW_DOWN if index + columns < 12 => index + columns,
            _ => index,
        };
        if self.selected != index {
            self.revision += 1;
            true
        } else {
            false
        }
    }

    pub fn selected_mode(&self) -> Option<Mode> {
        MODES.get(self.selected).copied()
    }

    pub fn present(&mut self, frame: &mut Frame) -> Result<(), Error> {
        if self.uploaded != self.revision {
            let atlas = dialog::atlas(&DEFINITIONS, &self.fresh, self.hover, self.selected);
            frame.upload_sprite_rgba8(
                SPRITE_ID,
                dialog::ATLAS_WIDTH as u32,
                dialog::ATLAS_HEIGHT as u32,
                &atlas,
            )?;
            self.uploaded = self.revision;
        }
        frame.begin_sprite_frame(rgba(0, 0, 0, 0))?;
        let mut quads = Vec::with_capacity(12);
        for i in 0..12 {
            let rect = Self::panel(i, frame.width(), frame.height());
            let col = i % dialog::ATLAS_COLUMNS;
            let row = i / dialog::ATLAS_COLUMNS;
            let u0 = col as f32 / dialog::ATLAS_COLUMNS as f32;
            let v0 = row as f32 / dialog::ATLAS_ROWS as f32;
            let u1 = (col + 1) as f32 / dialog::ATLAS_COLUMNS as f32;
            let v1 = (row + 1) as f32 / dialog::ATLAS_ROWS as f32;
            quads.push(SpriteQuad {
                sprite_id: SPRITE_ID,
                c0: SpriteCorner {
                    x: rect.x,
                    y: rect.y,
                    u: u0,
                    v: v0,
                },
                c1: SpriteCorner {
                    x: rect.x + rect.w,
                    y: rect.y,
                    u: u1,
                    v: v0,
                },
                c2: SpriteCorner {
                    x: rect.x + rect.w,
                    y: rect.y + rect.h,
                    u: u1,
                    v: v1,
                },
                c3: SpriteCorner {
                    x: rect.x,
                    y: rect.y + rect.h,
                    u: u0,
                    v: v1,
                },
                color_rgba: rgba(255, 255, 255, 255),
                source_over: true,
            });
        }
        frame.draw_sprite_quads(&quads)?;
        frame.publish(Damage::full(frame.width(), frame.height()))
    }
}
