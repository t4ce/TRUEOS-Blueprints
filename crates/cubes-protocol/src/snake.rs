//! Five-segment self-avoiding surface snake contract.
pub const SEGMENTS: usize = 5;
pub use crate::chain::{Cell, HALF, STEP_MS, adjacent, on_shell};
pub type State = crate::chain::State<SEGMENTS, false>;
pub type Step = crate::chain::Step<SEGMENTS, false>;
pub type Replica = crate::chain::Replica<SEGMENTS, false>;
