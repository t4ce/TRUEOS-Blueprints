#![no_std]

mod vcabi {
    pub use trueos_sys::vcabi::*;
}

pub mod ui2;
pub mod vgfx;
pub mod vsys;

pub mod prelude {
    pub use crate::ui2;
    pub use crate::vgfx;
    pub use crate::vsys;
}

