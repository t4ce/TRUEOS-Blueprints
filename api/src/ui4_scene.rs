//! UI4 scene-frame facade for Blueprint image and shader consumers.
//!
//! The implementation shares UI4's original Blueprint text transport, but
//! this name describes the general frame boundary used by shaded scenes.

pub use crate::ui4_solara_text::{
    CloseRequest, Damage, Error, Frame, KeyboardState, SkyboxRenderParams, SpriteCorner,
    SpriteQuad, rgba,
};
