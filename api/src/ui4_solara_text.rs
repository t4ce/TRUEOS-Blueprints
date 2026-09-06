//! Safe Blueprint facade for the experimental Solara text-row UI4 ABI.

use alloc::vec::Vec;

use crate::input::TrueosKeyboardOutputEvent;

pub const MAX_SCENE_TEXT_ROWS_PER_CALL: usize = 64;
const FONT_ID_STAMP_ONCE: u32 = 1 << 31;
const FONT_ID_TEXT_BACKBUFFER: u32 = 1 << 30;
const TEXT_BACKBUFFER_SPRITE_ID: u32 = u32::MAX;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontSize {
    pub native_scale: u32,
    pub target_pixels: u32,
}

/// One user-visible Shell font size and the warmed native source tier which
/// backs it. Intermediate entries deliberately retain their residual scale so
/// applications can move one smooth step at a time without inventing sizes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Shell2FontScaleStep {
    pub effective_pixels: u32,
    pub native_tier_pixels: u32,
    pub residual_milli: u32,
    pub columns_at_1280: u32,
    pub rows_at_720: u32,
    pub cache_eligible: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Font {
    Default = 1,
    NotoSansSc = 2,
    Inconsolata = 3,
    /// Optional user-installed terminal face. TRUEOS resolves this to the
    /// embedded Inconsolata face when JuliaMono is not present on TrueOSFS.
    JuliaMono = 4,
}

/// One Shell2-owned request for a GPU-resident, fully coloured glyph sprite.
/// The request does not expose glyph pixels to the Blueprint VM.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontSpriteRequest {
    pub font: Font,
    pub scalar: char,
    pub font_pixels: f32,
    pub color_rgba: u32,
}

/// Opaque per-window handle returned by [`Frame::request_font_sprite`].
/// Cache keys, fallback policy, and eviction remain the Blueprint app's job.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FontSpriteTicket(pub u64);

/// Nonblocking producer state for a requested glyph sprite.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FontSpriteStatus {
    Pending,
    Ready {
        sprite_id: u32,
        width: u32,
        height: u32,
        /// Tight tile origin relative to Shell2's logical terminal cell.
        origin_x: i32,
        origin_y: i32,
    },
    Failed,
}

/// Per-frame Escape policy. Frames close by default; choose
/// `DeliverToApplication` only when this frame handles Escape itself.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FrameEscapeKeyAction {
    Close = v::bp_abi::UI4_FRAME_ESCAPE_KEY_ACTION_CLOSE,
    DeliverToApplication = v::bp_abi::UI4_FRAME_ESCAPE_KEY_ACTION_DELIVER_TO_APPLICATION,
}

/// One Solara paint record in fixed viewport coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SceneTextRow<'a> {
    pub text: &'a str,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
}

/// One colored text run in a retained transparent RGBA8 font canvas.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontCanvasRow<'a> {
    pub text: &'a str,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
    pub color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Damage {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CursorSource {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
}

/// Pointer button bits shared with UI4's input broker.
pub const POINTER_BUTTON_PRIMARY: u32 = 1 << 0;
pub const POINTER_BUTTON_SECONDARY: u32 = 1 << 1;
pub const POINTER_BUTTON_MIDDLE: u32 = 1 << 2;

/// Longest label UI4 accepts for one context-menu row.
pub const MAX_MENU_LABEL_BYTES: usize = 64;
/// Most rows one invocation may carry.
pub const MAX_MENU_ENTRIES: usize = 16;

/// One context-menu row and the handler to run when it is chosen.
///
/// The handler is an ordinary function pointer over the caller's own state, so
/// a Blueprint registers behaviour per row and never handles an action id: the
/// ids are this module's private wire detail.
pub struct MenuEntry<'a, S> {
    label: &'a str,
    enabled: bool,
    on_click: fn(&mut S),
}

fn menu_noop<S>(_: &mut S) {}

impl<'a, S> MenuEntry<'a, S> {
    /// A selectable row which runs `on_click` when chosen.
    pub const fn new(label: &'a str, on_click: fn(&mut S)) -> Self {
        Self {
            label,
            enabled: true,
            on_click,
        }
    }

    /// A greyed row. UI4 shows the label but reports no selection for it.
    pub const fn disabled(label: &'a str) -> Self {
        Self {
            label,
            enabled: false,
            on_click: menu_noop::<S>,
        }
    }

    pub const fn label(&self) -> &'a str {
        self.label
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Why a context-menu invocation ended.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MenuCloseReason {
    Selected,
    Dismissed,
    Replaced,
    OwnerReleased,
    WindowClosed,
}

impl MenuCloseReason {
    const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Selected,
            2 => Self::Replaced,
            3 => Self::OwnerReleased,
            4 => Self::WindowClosed,
            _ => Self::Dismissed,
        }
    }
}

/// The worker lane this Blueprint was placed on.
///
/// The hypervisor gives every live Blueprint VM its own reserved lane, so this
/// is a stable, distinct small integer per running instance — enough for an
/// instance to place itself without being told who it is at the call site.
pub fn worker_slot() -> u32 {
    unsafe { v::bp_abi::trueos_cabi_wls_current_slot() }
}

/// Return the physical UI4/cursor output extent in pixels.
pub fn output_dimensions() -> Result<(u32, u32), Error> {
    let packed = unsafe { v::bp_abi::trueos_cabi_ui4_scene_output_dimensions() };
    let width = (packed >> 32) as u32;
    let height = packed as u32;
    if width == 0 || height == 0 {
        Err(Error::Ui4)
    } else {
        Ok((width, height))
    }
}

/// Kernel-provided slot-4 cursor presentations for a UI4 frame.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum CursorIcon {
    #[default]
    Default = 0,
    Loading = 1,
    ResizeHorizontal = 2,
    ResizeVertical = 3,
    ResizeDiagonal = 4,
    /// The selected frame paints its own cursor pixels.
    AppOwned = 5,
    /// UI4 outlines the stepped cell on its software-cursor plane.
    CellOutline = 6,
}

/// Frame-local presentation spacing for a software cursor. The advances use
/// 1/1024-pixel units so a fixed fractional glyph width stays aligned with the
/// frame's text grid while pointer input remains continuous.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CursorStep {
    pub origin_x: u32,
    pub origin_y: u32,
    pub cell_width_subpx: u32,
    pub cell_height_subpx: u32,
}

/// One selected-frame pointer event after UI4 hit testing and capture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PointerEvent {
    pub source: CursorSource,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i16,
    pub buttons_down: u32,
    pub buttons_pressed: u32,
    pub buttons_released: u32,
    pub combo_id: u32,
    pub vcursor: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PanPhase {
    Begin,
    Update,
    End,
}

/// One UI4 middle-button gesture already hit-tested and captured to this frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PanEvent {
    pub source: CursorSource,
    pub phase: PanPhase,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub combo_id: u32,
    pub vcursor: bool,
}

/// A UI4 maximize/restore request for this frame's backing extent.
///
/// The frame remains valid at its current size until the Blueprint chooses to
/// call [`Frame::resize`]. UI4 may smoothly scale those old pixels toward the
/// final geometry, but the application receives only this one final extent;
/// presentation-animation samples are never reported as resize events.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ResizeEvent {
    pub old_width: u32,
    pub old_height: u32,
    pub width: u32,
    pub height: u32,
}

/// Held HID usages for the keyboard routed to this selected UI4 frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct KeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_keyboard: bool,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
}

impl KeyboardState {
    pub fn is_down(&self, hid_usage: u8) -> bool {
        let key = hid_usage as usize;
        self.key_down_bits[key / 32] & (1u32 << (key % 32)) != 0
    }
}

/// One UI4 cursor/combo route scoped to this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputRoute {
    pub cursor: CursorSource,
    pub combo_id: u32,
    pub color_rgba: u32,
    pub selected_for_window: bool,
    pub application_focus: bool,
    pub vcursor: bool,
    pub keyboard: Option<KeyboardState>,
}

/// Camera basis and destination rectangle for the kernel RGB565 skybox
/// sampler. The source image is retained by the frame after upload.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SkyboxRenderParams {
    pub right_x: f32,
    pub right_y: f32,
    pub right_z: f32,
    pub up_x: f32,
    pub up_y: f32,
    pub up_z: f32,
    pub forward_x: f32,
    pub forward_y: f32,
    pub forward_z: f32,
    pub aspect_tan_half_fov_y: f32,
    pub tan_half_fov_y: f32,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
}

pub const PARTICLE_CRAFT_WIDTH: u32 = 640;
pub const PARTICLE_CRAFT_HEIGHT: u32 = 400;
pub const PARTICLE_CRAFT_PARAMS_VERSION: u32 = 1;
pub const PARTICLE_CRAFT_FLAG_RESET: u32 = 1 << 0;
pub const PARTICLE_CRAFT_FLAG_ATTRACTOR: u32 = 1 << 1;
pub const PARTICLE_CRAFT_FLAG_ORBIT: u32 = 1 << 2;
pub const PARTICLE_CRAFT_MAX_PARTICLES: u32 = 256;
pub const UI4_VISUAL_SOFT_CAP_HZ: u32 = 60;
pub const SHADERTOY_PARAMS_VERSION: u32 = 1;
pub const SHADERTOY_MANDELBROT: u32 = 1;
pub const SHADERTOY_CUBE_FIELD: u32 = 2;
pub const SHADERTOY_NGUYEN: u32 = 3;
pub const SHADERTOY_PALETTE_GRID: u32 = 4;
pub const SHADERTOY_COSMIC_STRANDS: u32 = 5;
pub const SHADERTOY_PROTEAN_CLOUDS: u32 = 6;
pub const SHADERTOY_AUDIO_VISUALIZER: u32 = 7;
pub const SHADERTOY_CPP_GALLERY: u32 = 8;
pub const SHADERTOY_AURORA: u32 = 9;
pub const SHADERTOY_JULIA: u32 = 10;
pub const SHADERTOY_SDF: u32 = 11;
pub const SHADERTOY_VORONOI: u32 = 12;
pub const SHADERTOY_RETRO_SUN: u32 = 13;
pub const SHADERTOY_HIGH_WISPS: u32 = 14;
pub const SHADERTOY_PARTICLE_CRAFT: u32 = 15;
/// Primary pointer held; accepted only by High Wisps.
pub const SHADERTOY_FLAG_PRIMARY_DOWN: u32 = 2;
/// F6 only: bypass automatic radial sampling for a full-resolution comparison.
pub const SHADERTOY_FLAG_NATIVE_RESOLUTION: u32 = 1;

/// Pointer-free controls for a kernel-reviewed ShaderToy catalog entry.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShadertoyParamsV1 {
    pub shader_id: u32,
    pub flags: u32,
    pub frame: u32,
    pub time_seconds: f32,
    pub delta_seconds: f32,
    pub frame_rate: f32,
    pub sample_rate: f32,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub click_x: f32,
    pub click_y: f32,
    pub date_year: f32,
    pub date_month: f32,
    pub date_day: f32,
    pub date_seconds: f32,
}

/// Pointer-free ParticleCraft v1 controls. Persistent particle state and GPU
/// addresses are retained by the kernel for this frame and never cross ABI.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParticleCraftParamsV1 {
    pub flags: u32,
    pub seed: u32,
    pub active_count: u32,
    pub dt_seconds: f32,
    pub time_seconds: f32,
    pub emitter_x: f32,
    pub emitter_y: f32,
    pub attractor_x: f32,
    pub attractor_y: f32,
    pub attraction: f32,
    pub swirl: f32,
    pub gravity_x: f32,
    pub gravity_y: f32,
    pub drag: f32,
    pub intensity: f32,
}

impl ParticleCraftParamsV1 {
    /// The Arc Forge preset used by both the Blueprint app and `cpp particle`.
    pub const fn arc_forge(time_seconds: f32, dt_seconds: f32, seed: u32) -> Self {
        Self {
            flags: PARTICLE_CRAFT_FLAG_ORBIT,
            seed,
            active_count: 128,
            dt_seconds,
            time_seconds,
            emitter_x: 320.0,
            emitter_y: 300.0,
            attractor_x: 320.0,
            attractor_y: 180.0,
            attraction: 94.0,
            swirl: 72.0,
            gravity_x: 0.0,
            gravity_y: 58.0,
            drag: 0.42,
            intensity: 1.0,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SpriteCorner {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

/// One ordered draw from a frame-retained straight-alpha RGBA sprite. Sprite
/// id zero is the frame-owned white pixel and is useful for solid rectangles.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SpriteQuad {
    pub sprite_id: u32,
    pub c0: SpriteCorner,
    pub c1: SpriteCorner,
    pub c2: SpriteCorner,
    pub c3: SpriteCorner,
    pub color_rgba: u32,
    pub source_over: bool,
}

impl Damage {
    pub const fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    NoBlueprintContext,
    NotFound,
    InvalidState,
    Font,
    Ui4,
    Busy,
    Unknown(i32),
}

/// Optional UI4 work requested at the coherent frame/session teardown point.
/// The default performs no capture or filesystem I/O.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CloseRequest {
    persist_final_frame: bool,
}

impl CloseRequest {
    /// Save the last published frame as `trueosfs:/finalframes/<app>.png`.
    /// A later close of the same app replaces the previous image.
    pub const fn persist_final_frame(mut self) -> Self {
        self.persist_final_frame = true;
        self
    }

    const fn flags(self) -> u32 {
        if self.persist_final_frame { 1 } else { 0 }
    }
}

pub struct Frame {
    window_id: u32,
    width: u32,
    height: u32,
}

impl Frame {
    pub fn open(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_open(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    /// Open a one-buffer snapshot frame.
    ///
    /// Each successful publish makes that allocation immutable. Calling
    /// `begin` again prepares a new one-buffer generation privately; the
    /// kernel swaps it into the same window only after the next publish.
    pub fn open_immutable(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_open_immutable(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    /// Open a triple-buffered scene frame for continuously shaded content.
    pub fn open_streaming(x: i32, y: i32, width: u32, height: u32) -> Result<Self, Error> {
        let window_id =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_open_streaming(x, y, width, height) };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    /// Open a GPU-only dirty/double visual frame with kernel-brokered cadence.
    /// Requests above 60 Hz are rejected at both API and kernel boundaries.
    pub fn open_visual(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        target_hz: u32,
    ) -> Result<Self, Error> {
        if target_hz == 0 || target_hz > UI4_VISUAL_SOFT_CAP_HZ {
            return Err(Error::Invalid);
        }
        let window_id = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_open_visual(x, y, width, height, target_hz)
        };
        if window_id == 0 {
            Err(Error::Ui4)
        } else {
            Ok(Self {
                window_id,
                width,
                height,
            })
        }
    }

    pub const fn window_id(&self) -> u32 {
        self.window_id
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn begin(&mut self, clear_rgba: u32) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_begin(self.window_id, clear_rgba) })
    }

    /// Give this frame a standing context menu, replacing any previous one.
    ///
    /// The frame owns the menu over its own pixels: UI4 raises it when a
    /// secondary click lands on this window, so the app never watches for the
    /// click itself. A window which registers nothing leaves that gesture to
    /// the kernel's desktop menu. UI4 owns rendering, hit testing, and
    /// teardown.
    ///
    /// Poll [`Frame::pump_context_menu`] with the same slice to run handlers.
    /// See [`Frame::clear_context_menu`] to give the gesture back.
    pub fn register_context_menu<S>(&mut self, entries: &[MenuEntry<'_, S>]) -> Result<(), Error> {
        if entries.is_empty() || entries.len() > MAX_MENU_ENTRIES {
            return Err(Error::Invalid);
        }
        let mut raw = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            if entry.label.is_empty() || entry.label.len() > MAX_MENU_LABEL_BYTES {
                return Err(Error::Invalid);
            }
            raw.push(v::bp_abi::TrueosUi4ContextMenuEntry {
                label_ptr: entry.label.as_ptr(),
                label_len: entry.label.len(),
                // Slice position is the wire identity, so a handler is found
                // again without the caller ever naming an id.
                action_id: index as u32 + 1,
                enabled: u32::from(entry.enabled),
            });
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_context_menu_register(
                self.window_id,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Drop this frame's standing menu. Secondary clicks over this window fall
    /// back to the kernel's desktop menu.
    pub fn clear_context_menu(&mut self) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_context_menu_register(self.window_id, core::ptr::null(), 0)
        })
    }

    /// Take one completed context-menu outcome and run the chosen row's
    /// handler against `state`.
    ///
    /// Returns the close reason when an invocation completed this call, or
    /// `None` while none has. Pass the same `entries` slice used to open the
    /// menu; a selection outside it runs no handler.
    pub fn pump_context_menu<S>(
        &mut self,
        entries: &[MenuEntry<'_, S>],
        state: &mut S,
    ) -> Result<Option<MenuCloseReason>, Error> {
        let mut event = v::bp_abi::TrueosUi4ContextMenuEvent::default();
        let taken = unsafe {
            v::bp_abi::trueos_cabi_ui4_context_menu_event_take(self.window_id, &mut event)
        };
        if taken == 1 {
            return Ok(None);
        }
        status(taken)?;
        if event.selected != 0
            && let Some(entry) = event
                .action_id
                .checked_sub(1)
                .and_then(|index| entries.get(index as usize))
            && entry.enabled
        {
            (entry.on_click)(state);
        }
        Ok(Some(MenuCloseReason::from_raw(event.reason)))
    }

    /// Take the next selected-frame pointer event. The click which selected
    /// this frame is absorbed by UI4 and is never returned here.
    pub fn take_pointer_event(&mut self) -> Result<Option<PointerEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4PointerEvent::default();
        let result = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_pointer_event_take(self.window_id, &mut raw)
        };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        let Ok(wheel) = i16::try_from(raw.wheel) else {
            return Err(Error::Invalid);
        };
        Ok(Some(PointerEvent {
            source: CursorSource {
                controller_id: raw.controller_id,
                slot_id: raw.slot_id,
                ep_target: raw.ep_target,
                hid_kind: raw.hid_kind,
            },
            x: raw.x,
            y: raw.y,
            local_x: raw.local_x,
            local_y: raw.local_y,
            dx: raw.dx,
            dy: raw.dy,
            wheel,
            buttons_down: raw.buttons_down,
            buttons_pressed: raw.buttons_pressed,
            buttons_released: raw.buttons_released,
            combo_id: raw.combo_id,
            vcursor: raw.vcursor != 0,
        }))
    }

    /// Take the next key/text transition already routed by UI4 to this exact
    /// owner and window. Complete text bursts are queued atomically; an
    /// upstream truncation is discarded before any part reaches this method.
    pub fn take_keyboard_event(&mut self) -> Result<Option<TrueosKeyboardOutputEvent>, Error> {
        let mut event = TrueosKeyboardOutputEvent::default();
        let result = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_keyboard_event_take(self.window_id, &mut event)
        };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        Ok(Some(event))
    }

    /// Take the next app-owned middle-button pan event for this frame.
    pub fn take_pan_event(&mut self) -> Result<Option<PanEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4PanEvent::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_pan_event_take(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        let phase = match raw.phase {
            1 => PanPhase::Begin,
            2 => PanPhase::Update,
            3 => PanPhase::End,
            _ => return Err(Error::Invalid),
        };
        Ok(Some(PanEvent {
            source: CursorSource {
                controller_id: raw.controller_id,
                slot_id: raw.slot_id,
                ep_target: raw.ep_target,
                hid_kind: raw.hid_kind,
            },
            phase,
            x: raw.x,
            y: raw.y,
            local_x: raw.local_x,
            local_y: raw.local_y,
            dx: raw.dx,
            dy: raw.dy,
            combo_id: raw.combo_id,
            vcursor: raw.vcursor != 0,
        }))
    }

    /// Take the next final UI4 maximize/restore extent for this frame.
    pub fn take_resize_event(&mut self) -> Result<Option<ResizeEvent>, Error> {
        let mut raw = v::bp_abi::TrueosUi4ResizeEvent::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_resize_event_take(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        Ok(Some(ResizeEvent {
            old_width: raw.old_width,
            old_height: raw.old_height,
            width: raw.width,
            height: raw.height,
        }))
    }

    /// Take the one-shot event proving that this window's first published
    /// frame crossed the compositor's physical SURFLIVE handoff.
    pub fn take_first_presentation(&mut self) -> Result<bool, Error> {
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_first_presentation_take(self.window_id) };
        match result {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(error_from_status(result)),
        }
    }

    /// Sample held keys only from the keyboard routed to this selected frame.
    /// Click/tap the frame first to establish the global UI4 selection.
    pub fn keyboard_state(&self) -> Result<Option<KeyboardState>, Error> {
        let mut raw = v::bp_abi::TrueosUi4KeyboardState::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_ui4_scene_keyboard_state(self.window_id, &mut raw) };
        if result == 1 {
            return Ok(None);
        }
        if result != 0 {
            return Err(error_from_status(result));
        }
        Ok(Some(KeyboardState {
            controller_id: raw.controller_id,
            slot_id: raw.slot_id,
            ep_target: raw.ep_target,
            combo_id: raw.combo_id,
            modifiers: raw.modifiers,
            source_kind: raw.source_kind,
            virtual_keyboard: raw.virtual_keyboard != 0,
            keys: raw.keys,
            ascii: raw.ascii,
            key_down_bits: raw.key_down_bits,
        }))
    }

    /// Snapshot every UI4 cursor/combo route with its exact selection state
    /// for this frame and its paired sanitized held keyboard state.
    pub fn input_routes(&self) -> Result<Vec<InputRoute>, Error> {
        let count = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_input_routes(self.window_id, core::ptr::null_mut(), 0)
        };
        if count < 0 {
            return Err(error_from_status(count as i32));
        }
        let capacity = usize::try_from(count).map_err(|_| Error::Invalid)?;
        let mut raw = alloc::vec![
            v::bp_abi::TrueosUi4InputRouteState::default();
            capacity
        ];
        if capacity == 0 {
            return Ok(Vec::new());
        }
        let returned = unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_input_routes(
                self.window_id,
                raw.as_mut_ptr(),
                raw.len() as u32,
            )
        };
        if returned < 0 {
            return Err(error_from_status(returned as i32));
        }
        raw.truncate(core::cmp::min(returned as usize, raw.len()));
        Ok(raw.into_iter().map(input_route_from_raw).collect())
    }

    /// Let a primary click both select this frame and reach its pointer queue.
    /// Useful for compact restore buttons; secondary dragging keeps its normal policy.
    pub fn set_primary_activation(&mut self, enabled: bool) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_primary_activation(self.window_id, enabled as u32) })
    }

    /// Read the broker's current position, including desktop drags.
    pub fn position(&self) -> Result<(i32, i32), Error> {
        let mut xy = [0i32; 2];
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_get_position(self.window_id, xy.as_mut_ptr()) })?;
        Ok((xy[0], xy[1]))
    }

    pub fn set_position(&mut self, x: i32, y: i32) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_frame_set_position(self.window_id, x, y) })
    }

    /// Set the opacity applied to every composited pixel of this frame.
    pub fn set_opacity(&mut self, opacity: u8) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_set_opacity(self.window_id, opacity as u32)
        })
    }

    /// Exclude this frame from UI4 cursor selection and pointer hit testing.
    pub fn set_hit_testable(&mut self, enabled: bool) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_set_hit_testable(self.window_id, enabled as u32)
        })
    }

    /// Set this frame's Escape policy without affecting any sibling window.
    pub fn set_escape_key_action(&mut self, action: FrameEscapeKeyAction) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_set_escape_key_action(
                self.window_id,
                action as u32,
            )
        })
    }

    /// Compatibility shorthand for a frame-wide [`CursorIcon::AppOwned`].
    pub fn set_custom_cursor(&mut self, enabled: bool) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_custom_cursor(self.window_id, u32::from(enabled))
        })
    }

    /// Snap only this frame's software-cursor presentation to a fixed cell
    /// grid. Passing `None` clears the policy. Pointer delivery, physical
    /// cursor movement, keyboard input, selection, and hit testing remain
    /// unchanged.
    pub fn set_cursor_step(&mut self, step: Option<CursorStep>) -> Result<(), Error> {
        let raw = step.map(|step| v::bp_abi::TrueosUi4CursorStep {
            origin_x: step.origin_x,
            origin_y: step.origin_y,
            cell_width_subpx: step.cell_width_subpx,
            cell_height_subpx: step.cell_height_subpx,
        });
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_cursor_step(
                self.window_id,
                raw.as_ref()
                    .map_or(core::ptr::null(), |step| step as *const _),
            )
        })
    }

    /// Set the selected frame's fallback cursor for every source which does
    /// not have its own override.
    pub fn set_cursor_icon(&mut self, icon: CursorIcon) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_cursor_icon(
                self.window_id,
                core::ptr::null(),
                icon as u32,
            )
        })
    }

    /// Override the cursor sprite for one of UI4's independent cursor routes.
    pub fn set_cursor_icon_for(
        &mut self,
        source: CursorSource,
        icon: CursorIcon,
    ) -> Result<(), Error> {
        let source = v::bp_abi::TrueosUi4CursorSource {
            controller_id: source.controller_id,
            slot_id: source.slot_id,
            ep_target: source.ep_target,
            hid_kind: source.hid_kind,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_set_cursor_icon(self.window_id, &source, icon as u32)
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_resize(self.window_id, width, height)
        })?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Copy a tightly packed full-frame RGBA8 image. Every input pixel must be
    /// opaque so it already satisfies UI4's premultiplied-alpha contract.
    pub fn write_opaque_rgba8(&mut self, rgba: &[u8]) -> Result<(), Error> {
        let expected = self.width as usize * self.height as usize * 4;
        if rgba.len() != expected || rgba.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX) {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_frame_write_opaque_rgba8(
                self.window_id,
                rgba.as_ptr(),
                rgba.len(),
            )
        })
    }

    /// Retain one decoded RGBA sprite in the UI4 frame's warm source set.
    /// Upload is normally performed once; later frames submit only quad data.
    pub fn upload_sprite_rgba8(
        &mut self,
        sprite_id: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), Error> {
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return Err(Error::Invalid);
        };
        if sprite_id == 0 || rgba.len() != expected {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_upload_rgba8(
                self.window_id,
                sprite_id,
                width,
                height,
                rgba.as_ptr(),
                rgba.len(),
            )
        })
    }

    /// Request production of one fully coloured GPU glyph sprite. This only
    /// submits work: it never waits, reads pixels back, or installs an
    /// app-independent font cache. The returned ticket is scoped to this UI4
    /// window; the caller owns its `(font, scalar, size, colour)` lookup.
    pub fn request_font_sprite(
        &mut self,
        request: FontSpriteRequest,
    ) -> Result<FontSpriteTicket, Error> {
        if !request.font_pixels.is_finite() || request.font_pixels <= 0.0 {
            return Err(Error::Invalid);
        }
        let mut ticket = 0_u64;
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_font_sprite_request_v1(
                self.window_id,
                request.font as u32,
                request.scalar as u32,
                request.font_pixels,
                request.color_rgba,
                &mut ticket,
            )
        })?;
        if ticket == 0 {
            return Err(Error::Invalid);
        }
        Ok(FontSpriteTicket(ticket))
    }

    /// Observe an asynchronous glyph request without waiting. A `Ready`
    /// sprite id may be submitted directly through [`Frame::draw_sprite_quads`]
    /// for this same window.
    pub fn font_sprite_status(
        &mut self,
        ticket: FontSpriteTicket,
    ) -> Result<FontSpriteStatus, Error> {
        if ticket.0 == 0 {
            return Err(Error::Invalid);
        }
        let mut raw = v::bp_abi::TrueosUi4FontSpriteStatusV1::default();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_font_sprite_status_v1(
                self.window_id,
                ticket.0,
                &mut raw,
            )
        })?;
        match raw.state {
            1 => Ok(FontSpriteStatus::Pending),
            2 if raw.sprite_id != 0 && raw.width != 0 && raw.height != 0 => {
                Ok(FontSpriteStatus::Ready {
                    sprite_id: raw.sprite_id,
                    width: raw.width,
                    height: raw.height,
                    origin_x: raw.origin_x,
                    origin_y: raw.origin_y,
                })
            }
            3 => Ok(FontSpriteStatus::Failed),
            _ => Err(Error::Invalid),
        }
    }

    /// Acquire a back buffer whose opaque clear is performed by the first GPU
    /// sprite batch rather than by a CPU full-frame paint.
    pub fn begin_sprite_frame(&mut self, clear_rgba: u32) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_frame_begin(self.window_id, clear_rgba)
        })
    }

    /// Acquire a back buffer for a full-frame GPU producer. No CPU clear is
    /// performed; the producer must overwrite the complete frame.
    pub fn begin_gpu_frame(&mut self) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_sprite_frame_begin(self.window_id, 0) })
    }

    /// Wait for the kernel's visual cadence deadline and acquire the next
    /// GPU-only back buffer. The VMCALL remains pending while it waits, so one
    /// call consumes one admission ticket without guest-side polling.
    pub fn begin_visual_gpu_frame(&mut self) -> Result<(), Error> {
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_visual_frame_begin(self.window_id) })
    }

    /// Render ordered retained sprites and solid rectangles into the active
    /// sprite-frame lease. Scenes larger than one hardware worklist are split
    /// by the kernel while preserving order.
    pub fn draw_sprite_quads(&mut self, quads: &[SpriteQuad]) -> Result<(), Error> {
        let raw = quads
            .iter()
            .map(|quad| v::bp_abi::TrueosUi4SpriteQuad {
                sprite_id: quad.sprite_id,
                c0_x: quad.c0.x,
                c0_y: quad.c0.y,
                c0_u: quad.c0.u,
                c0_v: quad.c0.v,
                c1_x: quad.c1.x,
                c1_y: quad.c1.y,
                c1_u: quad.c1.u,
                c1_v: quad.c1.v,
                c2_x: quad.c2.x,
                c2_y: quad.c2.y,
                c2_u: quad.c2.u,
                c2_v: quad.c2.v,
                c3_x: quad.c3.x,
                c3_y: quad.c3.y,
                c3_u: quad.c3.u,
                c3_v: quad.c3.v,
                color_rgba: quad.color_rgba,
                flags: u32::from(quad.source_over),
            })
            .collect::<Vec<_>>();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_sprite_quads(self.window_id, raw.as_ptr(), raw.len())
        })
    }

    /// Retain one tightly packed RGB565 equirectangular source for shaded
    /// rendering into this frame's back buffers.
    pub fn upload_skybox_rgb565(
        &mut self,
        width: u32,
        height: u32,
        rgb565: &[u8],
    ) -> Result<(), Error> {
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(2))
        else {
            return Err(Error::Invalid);
        };
        if rgb565.len() != expected {
            return Err(Error::Invalid);
        }
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_skybox_upload_rgb565(
                self.window_id,
                width,
                height,
                rgb565.as_ptr(),
                rgb565.len(),
            )
        })
    }

    /// Shade the retained skybox into the currently acquired UI4 back buffer.
    pub fn render_skybox_rgb565(&mut self, params: &SkyboxRenderParams) -> Result<(), Error> {
        let raw = v::bp_abi::TrueosUi4SkyboxRenderParams {
            right_x: params.right_x,
            right_y: params.right_y,
            right_z: params.right_z,
            up_x: params.up_x,
            up_y: params.up_y,
            up_z: params.up_z,
            forward_x: params.forward_x,
            forward_y: params.forward_y,
            forward_z: params.forward_z,
            aspect_tan_half_fov_y: params.aspect_tan_half_fov_y,
            tan_half_fov_y: params.tan_half_fov_y,
            rect_x: params.rect_x,
            rect_y: params.rect_y,
            rect_width: params.rect_width,
            rect_height: params.rect_height,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_skybox_render_rgb565(self.window_id, &raw)
        })
    }

    /// Advance the retained particle state and shade the complete current frame.
    ///
    /// Particle simulation and gather cost remain fixed at the native 640x400
    /// logical extent; a resized frame is covered from that retained result.
    pub fn render_particle_craft(&mut self, params: &ParticleCraftParamsV1) -> Result<(), Error> {
        if params.active_count == 0 || params.active_count > PARTICLE_CRAFT_MAX_PARTICLES {
            return Err(Error::Invalid);
        }
        let raw = v::bp_abi::TrueosUi4ParticleCraftParamsV1 {
            version: PARTICLE_CRAFT_PARAMS_VERSION,
            flags: params.flags,
            seed: params.seed,
            active_count: params.active_count,
            dt_seconds: params.dt_seconds,
            time_seconds: params.time_seconds,
            emitter_x: params.emitter_x,
            emitter_y: params.emitter_y,
            attractor_x: params.attractor_x,
            attractor_y: params.attractor_y,
            attraction: params.attraction,
            swirl: params.swirl,
            gravity_x: params.gravity_x,
            gravity_y: params.gravity_y,
            drag: params.drag,
            intensity: params.intensity,
        };
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_particle_craft_render(self.window_id, &raw)
        })
    }

    /// Register a Blueprint-owned shader package before its first render.
    /// The kernel authenticates the complete package, executable, SPIR-V and ABI
    /// against its own trust catalog. Source files travel with the package.
    pub fn register_shadertoy(&mut self, shader_id: u32, package: &[u8]) -> Result<(), Error> {
        if package.is_empty() || package.len() > u32::MAX as usize {
            return Err(Error::Invalid);
        }
        for (index, chunk) in package.chunks(2048).enumerate() {
            status(unsafe {
                v::bp_abi::trueos_cabi_ui4_scene_shadertoy_upload_v1(
                    self.window_id,
                    shader_id,
                    (index * 2048) as u32,
                    package.len() as u32,
                    chunk.as_ptr(),
                    chunk.len(),
                )
            })?;
        }
        Ok(())
    }

    /// Render and publish one immutable, offline-validated ShaderToy catalog
    /// artifact. ShaderToy always overwrites its complete visual-frame surface,
    /// so the completed compute lease is published with full-frame damage.
    pub fn render_shadertoy(&mut self, params: &ShadertoyParamsV1) -> Result<(), Error> {
        self.render_shadertoy_unpublished(params)?;
        self.publish_compute(Damage::full(self.width, self.height))
    }

    /// Render one reviewed ShaderToy artifact into the active visual-frame
    /// lease without publishing it. This is only for a caller that will make
    /// the matching explicit [`Self::publish_compute`] call itself.
    pub fn render_shadertoy_unpublished(
        &mut self,
        params: &ShadertoyParamsV1,
    ) -> Result<(), Error> {
        if !(1..=15).contains(&params.shader_id) {
            return Err(Error::Invalid);
        }
        let raw = v::bp_abi::TrueosUi4ShadertoyParamsV1 {
            version: SHADERTOY_PARAMS_VERSION,
            shader_id: params.shader_id,
            frame: params.frame,
            flags: params.flags,
            time_seconds: params.time_seconds,
            delta_seconds: params.delta_seconds,
            frame_rate: params.frame_rate,
            sample_rate: params.sample_rate,
            mouse_x: params.mouse_x,
            mouse_y: params.mouse_y,
            click_x: params.click_x,
            click_y: params.click_y,
            date_year: params.date_year,
            date_month: params.date_month,
            date_day: params.date_day,
            date_seconds: params.date_seconds,
        };
        status(unsafe { v::bp_abi::trueos_cabi_ui4_scene_shadertoy_render(self.window_id, &raw) })
    }

    /// Retain paint records without fitting their collective bounds to a stamp.
    ///
    /// The kernel builds analytical coverage on the asynchronous FontKernel
    /// task and keeps it GPU-VM resident behind this frame. Later paint passes
    /// reuse the same masks when only color or a common integral translation
    /// changed. The destination must be opened with [`Frame::open_streaming`]
    /// so the final compute release can cross UI4's triple-buffered scene
    /// handoff. Consumers should split large scenes into bounded calls.
    pub fn retain_text_scene(
        &mut self,
        font: Font,
        viewport: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        if viewport != (self.width, self.height) {
            return Err(Error::Invalid);
        }
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32,
                viewport.0,
                viewport.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Build or replace one persistent transparent premultiplied-RGBA8 font
    /// canvas. All colors and rows are submitted together; internal coverage
    /// layers are not exposed to the consumer. This call waits for FontKernel
    /// without holding a UI4 frame lease.
    pub fn retain_font_canvas(
        &mut self,
        font: Font,
        canvas: (u32, u32),
        rows: &[FontCanvasRow<'_>],
    ) -> Result<(), Error> {
        if canvas.0 == 0 || canvas.1 == 0 || rows.is_empty() {
            return Err(Error::Invalid);
        }
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4FontCanvasRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
                color_rgba: row.color_rgba,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_font_canvas(
                self.window_id,
                font as u32,
                canvas.0,
                canvas.1,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Describe the available portion of a viewport crop of the retained font
    /// canvas. If the frame is larger than the remaining canvas, the quad ends
    /// at the canvas edge and leaves the rest of the frame untouched. The quad
    /// uses source-over so transparent canvas pixels preserve the scene below
    /// it.
    pub fn font_canvas_quad(
        &self,
        canvas: (u32, u32),
        origin: (u32, u32),
    ) -> Result<SpriteQuad, Error> {
        if canvas.0 == 0 || canvas.1 == 0 || origin.0 >= canvas.0 || origin.1 >= canvas.1 {
            return Err(Error::Invalid);
        }
        let visible_width = self.width.min(canvas.0 - origin.0);
        let visible_height = self.height.min(canvas.1 - origin.1);
        let right = origin.0 + visible_width;
        let bottom = origin.1 + visible_height;
        let u0 = origin.0 as f32 / canvas.0 as f32;
        let v0 = origin.1 as f32 / canvas.1 as f32;
        let u1 = right as f32 / canvas.0 as f32;
        let v1 = bottom as f32 / canvas.1 as f32;
        Ok(SpriteQuad {
            sprite_id: TEXT_BACKBUFFER_SPRITE_ID,
            c0: SpriteCorner {
                x: 0.0,
                y: 0.0,
                u: u0,
                v: v0,
            },
            c1: SpriteCorner {
                x: visible_width as f32,
                y: 0.0,
                u: u1,
                v: v0,
            },
            c2: SpriteCorner {
                x: visible_width as f32,
                y: visible_height as f32,
                u: u1,
                v: v1,
            },
            c3: SpriteCorner {
                x: 0.0,
                y: visible_height as f32,
                u: u0,
                v: v1,
            },
            color_rgba: rgba(255, 255, 255, 255),
            source_over: true,
        })
    }

    /// Compose a retained font-canvas viewport into the active sprite frame.
    pub fn draw_font_canvas_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
    ) -> Result<(), Error> {
        let quad = self.font_canvas_quad(canvas, origin)?;
        self.draw_sprite_quads(core::slice::from_ref(&quad))
    }

    /// Acquire, compose, and publish one viewport of a warm font canvas.
    pub fn present_font_canvas_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
        clear_rgba: u32,
    ) -> Result<(), Error> {
        self.begin_sprite_frame(clear_rgba)?;
        self.draw_font_canvas_view(canvas, origin)?;
        self.publish(Damage::full(self.width, self.height))
    }

    /// Retain document text for one persistent, offscreen RGBA8 canvas.
    ///
    /// Call this one or more times between [`Frame::begin_sprite_frame`] and
    /// [`Frame::publish_text_backbuffer_view`]. The kernel retains analytical
    /// glyph coverage for every layer, materializes the complete canvas once,
    /// and then releases the masks. Subsequent viewport pans sample the warm
    /// RGBA8 canvas without rebuilding text or entering FontKernel again.
    pub fn retain_text_backbuffer(
        &mut self,
        font: Font,
        canvas: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32 | FONT_ID_TEXT_BACKBUFFER,
                canvas.0,
                canvas.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Crop the persistent text canvas into the active UI4 sprite-frame lease
    /// and publish it. No font work occurs after the first successful call.
    pub fn publish_text_backbuffer_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
    ) -> Result<(), Error> {
        let Some(right) = origin.0.checked_add(self.width) else {
            return Err(Error::Invalid);
        };
        let Some(bottom) = origin.1.checked_add(self.height) else {
            return Err(Error::Invalid);
        };
        if canvas.0 == 0 || canvas.1 == 0 || right > canvas.0 || bottom > canvas.1 {
            return Err(Error::Invalid);
        }
        let u0 = origin.0 as f32 / canvas.0 as f32;
        let v0 = origin.1 as f32 / canvas.1 as f32;
        let u1 = right as f32 / canvas.0 as f32;
        let v1 = bottom as f32 / canvas.1 as f32;
        let quad = SpriteQuad {
            sprite_id: TEXT_BACKBUFFER_SPRITE_ID,
            c0: SpriteCorner {
                x: 0.0,
                y: 0.0,
                u: u0,
                v: v0,
            },
            c1: SpriteCorner {
                x: self.width as f32,
                y: 0.0,
                u: u1,
                v: v0,
            },
            c2: SpriteCorner {
                x: self.width as f32,
                y: self.height as f32,
                u: u1,
                v: v1,
            },
            c3: SpriteCorner {
                x: 0.0,
                y: self.height as f32,
                u: u0,
                v: v1,
            },
            color_rgba: rgba(255, 255, 255, 255),
            source_over: false,
        };
        self.draw_sprite_quads(core::slice::from_ref(&quad))?;
        self.publish(Damage::full(self.width, self.height))
    }

    /// Acquire a new UI4 lease, crop the already-materialized text canvas, and
    /// publish it. This is the complete steady-state pan operation.
    pub fn present_text_backbuffer_view(
        &mut self,
        canvas: (u32, u32),
        origin: (u32, u32),
        clear_rgba: u32,
    ) -> Result<(), Error> {
        self.begin_sprite_frame(clear_rgba)?;
        self.publish_text_backbuffer_view(canvas, origin)
    }

    /// Asynchronously rasterize fixed text once into the leased UI4 frame.
    ///
    /// This is the economical path for static labels or infrequently refreshed
    /// dashboards. Multiple calls before publication become ordered layers in
    /// one FontKernel request; no intermediate full-frame RGBA allocation or
    /// follow-up UI4 compositor pass is required.
    pub fn stamp_text_scene(
        &mut self,
        font: Font,
        viewport: (u32, u32),
        color_rgba: u32,
        rows: &[SceneTextRow<'_>],
    ) -> Result<(), Error> {
        if viewport != (self.width, self.height) {
            return Err(Error::Invalid);
        }
        let raw: Vec<_> = rows
            .iter()
            .map(|row| v::bp_abi::TrueosUi4SolaraSceneTextRow {
                text_ptr: row.text.as_ptr(),
                text_len: row.text.len(),
                x: row.x,
                y: row.y,
                font_pixels: row.font_pixels,
            })
            .collect();
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_text_scene(
                self.window_id,
                font as u32 | FONT_ID_STAMP_ONCE,
                viewport.0,
                viewport.1,
                color_rgba,
                raw.as_ptr(),
                raw.len(),
            )
        })
    }

    /// Publish the active visual/compute frame. The producer's completion
    /// fence is held by UI4 and bound to this canvas' current lease; neither
    /// Solara nor a 3D render-scene path participates in the handoff.
    pub fn publish_compute(&mut self, damage: Damage) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_scene_compute_frame_publish(
                self.window_id,
                damage.x,
                damage.y,
                damage.width,
                damage.height,
            )
        })
    }

    /// Legacy broad publisher for callers that publish CPU or text content
    /// through this UI4 canvas. Compute producers should use
    /// [`Self::publish_compute`] so their release semantics stay explicit.
    pub fn publish(&mut self, damage: Damage) -> Result<(), Error> {
        status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_frame_publish(
                self.window_id,
                damage.x,
                damage.y,
                damage.width,
                damage.height,
            )
        })
    }

    /// Close this UI4 frame with optional teardown work. If final-frame
    /// persistence is requested without a writable TRUEOSFS root, close still
    /// succeeds and the capture is skipped.
    pub fn close(mut self, request: CloseRequest) -> Result<(), Error> {
        let result = status(unsafe {
            v::bp_abi::trueos_cabi_ui4_solara_frame_close_requested(self.window_id, request.flags())
        });
        if result.is_ok() {
            self.window_id = 0;
        }
        result
    }
}

fn input_route_from_raw(raw: v::bp_abi::TrueosUi4InputRouteState) -> InputRoute {
    let keyboard =
        (raw.flags & v::bp_abi::UI4_INPUT_ROUTE_KEYBOARD_PRESENT != 0).then_some(KeyboardState {
            controller_id: raw.keyboard_controller_id,
            slot_id: raw.keyboard_slot_id,
            ep_target: raw.keyboard_ep_target,
            combo_id: raw.combo_id,
            modifiers: raw.keyboard_modifiers,
            source_kind: raw.keyboard_source_kind,
            virtual_keyboard: raw.virtual_keyboard != 0,
            keys: raw.keys,
            ascii: raw.ascii,
            key_down_bits: raw.key_down_bits,
        });
    InputRoute {
        cursor: CursorSource {
            controller_id: raw.cursor_controller_id,
            slot_id: raw.cursor_slot_id,
            ep_target: raw.cursor_ep_target,
            hid_kind: raw.cursor_hid_kind,
        },
        combo_id: raw.combo_id,
        color_rgba: raw.color_rgba,
        selected_for_window: raw.flags & v::bp_abi::UI4_INPUT_ROUTE_SELECTED_FOR_WINDOW != 0,
        application_focus: raw.flags & v::bp_abi::UI4_INPUT_ROUTE_APP_FOCUS != 0,
        vcursor: raw.flags & v::bp_abi::UI4_INPUT_ROUTE_VCURSOR != 0,
        keyboard,
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if self.window_id != 0 {
            let _ = unsafe { v::bp_abi::trueos_cabi_ui4_solara_frame_close(self.window_id) };
            self.window_id = 0;
        }
    }
}

pub fn font_sizes() -> Result<Vec<FontSize>, Error> {
    let count = unsafe { v::bp_abi::trueos_cabi_ui4_solara_font_sizes(core::ptr::null_mut(), 0) };
    if count < 0 {
        return Err(error_from_status(count as i32));
    }
    let mut raw = Vec::with_capacity(count as usize);
    raw.resize(
        count as usize,
        v::bp_abi::TrueosUi4SolaraFontSize::default(),
    );
    let written =
        unsafe { v::bp_abi::trueos_cabi_ui4_solara_font_sizes(raw.as_mut_ptr(), raw.len()) };
    if written < 0 {
        return Err(error_from_status(written as i32));
    }
    raw.truncate((written as usize).min(raw.len()));
    Ok(raw
        .into_iter()
        .map(|size| FontSize {
            native_scale: size.native_scale,
            target_pixels: size.target_pixels,
        })
        .collect())
}

/// Return UI4's ordered Shell font-size ladder.
///
/// The order is the interaction contract: moving by one entry includes the
/// residual-scaled sizes between native cache tiers.
pub fn shell2_font_scale_steps() -> Result<Vec<Shell2FontScaleStep>, Error> {
    let count =
        unsafe { v::bp_abi::trueos_cabi_ui4_shell2_font_scale_steps_v1(core::ptr::null_mut(), 0) };
    if count < 0 {
        return Err(error_from_status(count as i32));
    }
    let mut raw = Vec::with_capacity(count as usize);
    raw.resize(
        count as usize,
        v::bp_abi::TrueosUi4Shell2FontScaleStep::default(),
    );
    let written = unsafe {
        v::bp_abi::trueos_cabi_ui4_shell2_font_scale_steps_v1(raw.as_mut_ptr(), raw.len())
    };
    if written < 0 {
        return Err(error_from_status(written as i32));
    }
    raw.truncate((written as usize).min(raw.len()));
    Ok(raw
        .into_iter()
        .map(|step| Shell2FontScaleStep {
            effective_pixels: step.effective_pixels,
            native_tier_pixels: step.native_tier_pixels,
            residual_milli: step.residual_milli,
            columns_at_1280: step.columns_at_1280,
            rows_at_720: step.rows_at_720,
            cache_eligible: step.cache_eligible != 0,
        })
        .collect())
}

/// Pack conventional RGBA channels into the ABI's little-endian `u32` form.
pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    u32::from_le_bytes([red, green, blue, alpha])
}

fn status(code: i32) -> Result<(), Error> {
    if code == 0 {
        Ok(())
    } else {
        Err(error_from_status(code))
    }
}

fn error_from_status(code: i32) -> Error {
    match code {
        -1 => Error::Invalid,
        -2 => Error::NoBlueprintContext,
        -3 => Error::NotFound,
        -4 => Error::InvalidState,
        -5 => Error::Font,
        -6 => Error::Ui4,
        -7 => Error::Busy,
        other => Error::Unknown(other),
    }
}
