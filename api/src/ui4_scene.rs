//! UI4 scene-frame facade for Blueprint image and shader consumers.
//!
//! The implementation shares UI4's original Blueprint text transport, but
//! this name describes the general frame boundary used by shaded scenes.

pub use crate::ui4_solara_text::{
    CloseRequest, CursorIcon, CursorSource, CursorStep, Damage, Error, Font, FontCanvasRow,
    FontSize, FontSpriteRequest, FontSpriteStatus, FontSpriteTicket, Frame, InputRoute,
    KeyboardState, MAX_MENU_ENTRIES, MAX_MENU_LABEL_BYTES, MenuCloseReason, MenuEntry,
    PARTICLE_CRAFT_FLAG_ATTRACTOR, PARTICLE_CRAFT_FLAG_ORBIT, PARTICLE_CRAFT_FLAG_RESET,
    PARTICLE_CRAFT_HEIGHT, PARTICLE_CRAFT_MAX_PARTICLES, PARTICLE_CRAFT_PARAMS_VERSION,
    PARTICLE_CRAFT_WIDTH, POINTER_BUTTON_MIDDLE, POINTER_BUTTON_PRIMARY, POINTER_BUTTON_SECONDARY,
    PanEvent, PanPhase, ParticleCraftParamsV1, PointerEvent, ResizeEvent, SHADERTOY_CUBE_FIELD,
    SHADERTOY_PROTEAN_CLOUDS, SHADERTOY_COSMIC_STRANDS, SHADERTOY_MANDELBROT, SHADERTOY_NGUYEN, SHADERTOY_PALETTE_GRID,
    SHADERTOY_PARAMS_VERSION,
    SceneTextRow,
    ShadertoyParamsV1, Shell2FontScaleStep, SkyboxRenderParams, SpriteCorner, SpriteQuad,
    UI4_VISUAL_SOFT_CAP_HZ, font_sizes, output_dimensions, rgba, shell2_font_scale_steps,
    worker_slot,
};
