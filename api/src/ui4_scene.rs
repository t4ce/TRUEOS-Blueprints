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
    UI4_VISUAL_SOFT_CAP_HZ, font_sizes, output_dimensions, rgba, set_display_gamma_ramp,
    shell2_font_scale_steps, worker_slot,
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

/// Maximum exact pixel extent for a kernel-owned compact window image.
pub const MAX_COLLAPSE_IMAGE_SIZE: u32 = 256;

impl Frame {
    /// Collapse an opted-in frame, preserving its producer and logical extent.
    pub fn collapse(&mut self) -> Result<(), Error> {
        let rc = unsafe { v::bp_abi::trueos_cabi_ui4_scene_collapse_v1(self.window_id()) };
        crate::ui4_solara_text::status(rc)
    }

    /// Enable the standard collapse/restore menu and click-to-restore behavior.
    /// Copies tightly packed straight-alpha RGBA8, at its exact size (up to
    /// 256x256). Call again to update the icon. No menu or input pump is needed;
    /// the producer's frame, layers, and logical extent remain intact.
    pub fn enable_collapse(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), Error> {
        if !(1..=MAX_COLLAPSE_IMAGE_SIZE).contains(&width)
            || !(1..=MAX_COLLAPSE_IMAGE_SIZE).contains(&height)
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err(Error::Invalid);
        }
        let rc = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_enable_collapse_v1(
                self.window_id(),
                width,
                height,
                rgba.as_ptr(),
                rgba.len(),
            )
        };
        crate::ui4_solara_text::status(rc)
    }
}

/// Convert a physical size through the active output's EDID metrics.
/// Returns an error when physical display dimensions are unavailable.
pub fn physical_extent(width_mm: u32, height_mm: u32) -> Result<(u32, u32), Error> {
    let packed = unsafe { v::bp_abi::trueos_cabi_ui4_scene_physical_extent_v1(width_mm, height_mm) };
    let extent = ((packed >> 32) as u32, packed as u32);
    if extent.0 == 0 || extent.1 == 0 { Err(Error::Ui4) } else { Ok(extent) }
}

/// Kernel-owned web-image completion metadata; image bytes stay in the kernel.
pub use v::bp_abi::TrueosUi4HullImageInfoV1 as HullImageInfo;
pub const HULL_IMAGE_PENDING: u32 = v::bp_abi::UI4_HULL_IMAGE_PENDING;
pub const HULL_IMAGE_READY: u32 = v::bp_abi::UI4_HULL_IMAGE_READY;
pub const HULL_IMAGE_FAILED: u32 = v::bp_abi::UI4_HULL_IMAGE_FAILED;

/// Queue a canonical image URL for this foreground window and page generation.
/// Pending/ready tickets remain owned until release or reset. Target dimensions
/// bound the decode; poll for the actual retained sprite dimensions.
pub fn hull_image_request(
    window_id: u32,
    url: &str,
    cache_root: &str,
    page_host: &str,
    width: u32,
    height: u32,
    generation: u64,
) -> Result<u32, Error> {
    if window_id == 0 || url.is_empty() || url.len() > 8_192
        || cache_root.is_empty() || cache_root.len() > 2_048
        || page_host.is_empty() || page_host.len() > 253
        || width == 0 || height == 0 || width > 4_096 || height > 4_096
    {
        return Err(Error::Invalid);
    }
    let ticket = unsafe {
        v::bp_abi::trueos_cabi_ui4_hull_image_request_v1(
            window_id, url.as_ptr(), url.len(), cache_root.as_ptr(), cache_root.len(),
            page_host.as_ptr(), page_host.len(), width, height, generation,
        )
    };
    if ticket > 0 { Ok(ticket as u32) }
    else if ticket == 0 { Err(Error::InvalidState) }
    else { crate::ui4_solara_text::status(ticket).map(|()| 0) }
}

/// Poll without acquiring a frame lease or waiting for fetch/decode. READY's
/// sprite_id can be used directly in SpriteQuad until release/reset/close.
pub fn hull_image_status(window_id: u32, ticket: u32) -> Result<HullImageInfo, Error> {
    if window_id == 0 || ticket == 0 { return Err(Error::Invalid); }
    let mut info = HullImageInfo::default();
    crate::ui4_solara_text::status(unsafe {
        v::bp_abi::trueos_cabi_ui4_hull_image_status_v1(window_id, ticket, &mut info)
    })?;
    Ok(info)
}

/// Cancel a pending ticket or retire its retained source after GPU completion.
pub fn hull_image_release(window_id: u32, ticket: u32) -> Result<(), Error> {
    if window_id == 0 || ticket == 0 { return Err(Error::Invalid); }
    crate::ui4_solara_text::status(unsafe {
        v::bp_abi::trueos_cabi_ui4_hull_image_release_v1(window_id, ticket)
    })
}

/// Cancel all prior page requests/resources and select the next generation.
pub fn hull_image_reset(window_id: u32, generation: u64) -> Result<(), Error> {
    if window_id == 0 { return Err(Error::Invalid); }
    crate::ui4_solara_text::status(unsafe {
        v::bp_abi::trueos_cabi_ui4_hull_image_reset_v1(window_id, generation)
    })
}
