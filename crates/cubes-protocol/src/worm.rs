//! Nine-segment ghost worm: surface steps or a jump through the center.
pub const SEGMENTS: usize = 9;
pub use crate::chain::{Cell, HALF, STEP_MS, on_shell, tunnel};
pub type State = crate::chain::State<SEGMENTS, true>;
pub type Step = crate::chain::Step<SEGMENTS, true>;
pub type Replica = crate::chain::Replica<SEGMENTS, true>;
