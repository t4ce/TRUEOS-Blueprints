//! UI4 scene-frame facade for Blueprint image and shader consumers.
//!
//! The implementation shares UI4's original Blueprint text transport, but
//! this name describes the general frame boundary used by shaded scenes.

pub use crate::ui4_solara_text::{
    CloseRequest, CursorIcon, CursorSource, Damage, Error, Frame, InputRoute, KeyboardState,
    PARTICLE_CRAFT_FLAG_ATTRACTOR, PARTICLE_CRAFT_FLAG_ORBIT, PARTICLE_CRAFT_FLAG_RESET,
    PARTICLE_CRAFT_HEIGHT, PARTICLE_CRAFT_MAX_PARTICLES, PARTICLE_CRAFT_PARAMS_VERSION,
    PARTICLE_CRAFT_WIDTH, ParticleCraftParamsV1, PointerEvent, ResizeEvent, SkyboxRenderParams,
    SpriteCorner, SpriteQuad, output_dimensions, rgba,
};
