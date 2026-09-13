//! Shared stable-slot stream for surface chains and ghost chains with tunnels.
pub const STEP_MS: u64 = 200;
pub const HALF: i16 =
    (crate::gallery::CENTER_CUBE_SIDE_C1 * crate::gallery::CENTER_CUBES_PER_AXIS / 2) as i16;
pub type Cell = [i16; 3];
pub fn on_shell(p: Cell) -> bool {
    p.iter().all(|&v| (-HALF - 1..=HALF).contains(&v))
        && p.iter().any(|&v| v == -HALF - 1 || v == HALF)
}
pub fn adjacent(a: Cell, b: Cell) -> bool {
    (0..3)
        .map(|i| (a[i] as i32 - b[i] as i32).abs())
        .sum::<i32>()
        == 1
}
fn newer(a: u32, b: u32) -> bool {
    let d = a.wrapping_sub(b);
    d != 0 && d < 1 << 31
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State<const SEGMENTS: usize, const GHOST: bool> {
    pub gallery: u32,
    pub epoch: u32,
    pub tick: u32,
    pub head: u8,
    pub cells: [Cell; SEGMENTS],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step<const SEGMENTS: usize, const GHOST: bool> {
    pub gallery: u32,
    pub epoch: u32,
    pub tick: u32,
    pub slot: u8,
    pub cell: Cell,
}
fn header(b: &[u8]) -> (u32, u32, u32, u8) {
    (
        u32::from_le_bytes(b[..4].try_into().unwrap()),
        u32::from_le_bytes(b[4..8].try_into().unwrap()),
        u32::from_le_bytes(b[8..12].try_into().unwrap()),
        b[12],
    )
}
fn encode_header(b: &mut [u8], gallery: u32, epoch: u32, tick: u32, slot: u8) {
    b[..4].copy_from_slice(&gallery.to_le_bytes());
    b[4..8].copy_from_slice(&epoch.to_le_bytes());
    b[8..12].copy_from_slice(&tick.to_le_bytes());
    b[12] = slot;
}
fn cell(b: &[u8]) -> Cell {
    core::array::from_fn(|a| i16::from_le_bytes(b[a * 2..a * 2 + 2].try_into().unwrap()))
}
fn encode_cell(b: &mut [u8], p: Cell) {
    for a in 0..3 {
        b[a * 2..a * 2 + 2].copy_from_slice(&p[a].to_le_bytes());
    }
}
impl<const SEGMENTS: usize, const GHOST: bool> State<SEGMENTS, GHOST> {
    pub fn valid(self) -> bool {
        (self.head as usize) < SEGMENTS
            && self.cells.iter().all(|&p| on_shell(p))
            && (GHOST || (0..SEGMENTS).all(|i| !self.cells[i + 1..].contains(&self.cells[i])))
            && (1..SEGMENTS).all(|i| {
                connected::<GHOST>(
                    self.cells[(self.head as usize + i) % SEGMENTS],
                    self.cells[(self.head as usize + i + 1) % SEGMENTS],
                )
            })
    }
    pub fn encode(self) -> alloc::vec::Vec<u8> {
        let mut b = alloc::vec![0; 13 + 6 * SEGMENTS];
        encode_header(&mut b, self.gallery, self.epoch, self.tick, self.head);
        for (r, p) in b[13..].chunks_exact_mut(6).zip(self.cells) {
            encode_cell(r, p);
        }
        b
    }
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() != 13 + 6 * SEGMENTS {
            return None;
        }
        let (gallery, epoch, tick, head) = header(b);
        let s = Self {
            gallery,
            epoch,
            tick,
            head,
            cells: core::array::from_fn(|i| cell(&b[13 + i * 6..19 + i * 6])),
        };
        s.valid().then_some(s)
    }
    pub fn apply(&mut self, step: Step<SEGMENTS, GHOST>) -> bool {
        if step.gallery != self.gallery
            || step.epoch != self.epoch
            || step.tick != self.tick.wrapping_add(1)
            || step.slot as usize != (self.head as usize + 1) % SEGMENTS
        {
            return false;
        }
        let mut next = *self;
        next.tick = step.tick;
        next.head = step.slot;
        next.cells[step.slot as usize] = step.cell;
        if !next.valid() {
            return false;
        }
        *self = next;
        true
    }
}
impl<const SEGMENTS: usize, const GHOST: bool> Step<SEGMENTS, GHOST> {
    pub fn encode(self) -> [u8; 19] {
        let mut b = [0; 19];
        encode_header(&mut b, self.gallery, self.epoch, self.tick, self.slot);
        encode_cell(&mut b[13..], self.cell);
        b
    }
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() != 19 {
            return None;
        }
        let (gallery, epoch, tick, slot) = header(b);
        let cell = cell(&b[13..]);
        (slot < SEGMENTS as u8 && on_shell(cell)).then_some(Self {
            gallery,
            epoch,
            tick,
            slot,
            cell,
        })
    }
}
#[derive(Default)]
pub struct Replica<const SEGMENTS: usize, const GHOST: bool> {
    pub state: Option<State<SEGMENTS, GHOST>>,
    pub needs_snapshot: bool,
}
impl<const SEGMENTS: usize, const GHOST: bool> Replica<SEGMENTS, GHOST> {
    pub fn snapshot(&mut self, state: State<SEGMENTS, GHOST>) -> bool {
        if !state.valid() {
            return false;
        }
        if let Some(old) = self.state {
            if state == old {
                self.needs_snapshot = false;
                return false;
            }
            if state.epoch == old.epoch
                && state.gallery == old.gallery
                && !newer(state.tick, old.tick)
            {
                return false;
            }
        }
        self.state = Some(state);
        self.needs_snapshot = false;
        true
    }
    pub fn step(&mut self, step: Step<SEGMENTS, GHOST>) -> bool {
        if let Some(state) = self.state.as_mut() {
            if state.epoch == step.epoch
                && state.gallery == step.gallery
                && !newer(step.tick, state.tick)
            {
                return false;
            }
            if state.apply(step) {
                self.needs_snapshot = false;
                return true;
            }
        }
        self.needs_snapshot = true;
        false
    }
}

/// A tunnel jumps between opposite face cells, through the solid interior.
pub fn tunnel(a: Cell, b: Cell) -> bool {
    (0..3).any(|axis| {
        a[axis] + b[axis] == -1
            && (a[axis] == -HALF - 1 || a[axis] == HALF)
            && (0..3).all(|i| i == axis || (a[i] == b[i] && (-HALF..HALF).contains(&a[i])))
    })
}
fn connected<const GHOST: bool>(a: Cell, b: Cell) -> bool {
    adjacent(a, b) || (GHOST && tunnel(a, b))
}
