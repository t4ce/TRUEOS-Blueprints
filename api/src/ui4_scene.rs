//! UI4 scene-frame facade for Blueprint image and shader consumers.
//!
//! The implementation shares UI4's original Blueprint text transport, but
//! this name describes the general frame boundary used by shaded scenes.

pub use crate::ui4_solara_text::{
    BackgroundLayer, CloseRequest, CursorIcon, CursorSource, CursorStep, Damage, Error, Font,
    FontCanvasRow, FontSize, FontSpriteRequest, FontSpriteStatus, FontSpriteTicket, Frame,
    InputRoute, KeyboardState, MAX_MENU_ENTRIES, MAX_MENU_LABEL_BYTES, MenuCloseReason, MenuEntry,
    PARTICLE_CRAFT_FLAG_ATTRACTOR, PARTICLE_CRAFT_FLAG_ORBIT, PARTICLE_CRAFT_FLAG_RESET,
    PARTICLE_CRAFT_HEIGHT, PARTICLE_CRAFT_MAX_PARTICLES, PARTICLE_CRAFT_PARAMS_VERSION,
    PARTICLE_CRAFT_WIDTH, POINTER_BUTTON_MIDDLE, POINTER_BUTTON_PRIMARY, POINTER_BUTTON_SECONDARY,
    PanEvent, PanPhase, ParticleCraftParamsV1, PointerEvent, ResizeEvent,
    SHADERTOY_AUDIO_VISUALIZER, SHADERTOY_AURORA, SHADERTOY_COSMIC_STRANDS, SHADERTOY_CPP_GALLERY,
    SHADERTOY_CUBE_FIELD, SHADERTOY_FLAG_NATIVE_RESOLUTION, SHADERTOY_FLAG_PRIMARY_DOWN,
    SHADERTOY_HIGH_WISPS, SHADERTOY_JULIA, SHADERTOY_MANDELBROT, SHADERTOY_NGUYEN,
    SHADERTOY_PALETTE_GRID, SHADERTOY_PARAMS_VERSION, SHADERTOY_PARTICLE_CRAFT,
    SHADERTOY_PROTEAN_CLOUDS, SHADERTOY_RETRO_SUN, SHADERTOY_SDF, SHADERTOY_VORONOI, SceneTextRow,
    ShadertoyParamsV1, Shell2FontScaleStep, SkyboxRenderParams, SpriteCorner, SpriteQuad,
    UI4_VISUAL_SOFT_CAP_HZ, font_sizes, output_dimensions, rgba, shell2_font_scale_steps,
    worker_slot,
};

/// Fade the entire primary display through its hardware gamma LUT. The kernel
/// restores the previous LUT at zero and on Blueprint teardown. One owner at a time.
pub fn display_fade(window_id: u32, amount: f32) -> Result<(), Error> {
    if !amount.is_finite() || !(-1.0..=1.0).contains(&amount) { return Err(Error::Invalid); }
    // Float-to-integer casts truncate toward zero; bias by half a unit to
    // preserve round-to-nearest (ties away from zero) without std float methods.
    let scaled = amount * 65535.0;
    let encoded = (scaled + if scaled < 0.0 { -0.5 } else { 0.5 }) as i32;
    let rc = unsafe { v::bp_abi::trueos_cabi_ui4_scene_display_fade_v1(window_id, encoded) };
    match rc { 0 => Ok(()), -1 => Err(Error::Invalid), -2 => Err(Error::NoBlueprintContext),
        code => Err(Error::Unknown(code)) }
}
