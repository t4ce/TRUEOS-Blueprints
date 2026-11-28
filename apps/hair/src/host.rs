use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr, thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use libloading::Library;

use crate::{
    Controls, TILT_LIMIT,
    hair_rope::{HairMotion, WIND_HEADING_COUNT},
    scene::headband_fixation_percent,
};

const TARGET_HZ: u64 = 45;
const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / TARGET_HZ);
const HELP: &str =
    "Hold A/D to orbit · Mouse left/right spins · down tilts forward · up tilts back";
const ROD_KEYS: &str = "Tab hull look · 1 wind strength · 2 wind direction · Shift steps backward";
const WATER_KEYS: &str = "I/K volume  J/L flow speed  U/O viscosity";
const RADIANS_PER_PIXEL: f32 = 0.008;
const ORBIT_RADIANS_PER_SECOND: f32 = std::f32::consts::FRAC_PI_4;
const WIND_STRENGTH_MAX: u8 = 100;
const WIND_STRENGTH_STEP: u8 = 10;

#[derive(Default)]
struct OrbitKeys {
    left: bool,
    right: bool,
}

impl OrbitKeys {
    fn set(&mut self, scancode: i32, pressed: bool) {
        match scancode {
            KEY_A => self.left = pressed,
            KEY_D => self.right = pressed,
            _ => (),
        }
    }

    fn advance(&self, angle: &mut f32, elapsed: Duration) {
        let direction = i8::from(self.right) - i8::from(self.left);
        if direction != 0 {
            let elapsed = elapsed.as_secs_f32().min(0.1);
            *angle = (*angle + f32::from(direction) * elapsed * ORBIT_RADIANS_PER_SECOND)
                .rem_euclid(std::f32::consts::TAU);
        }
    }
}

pub(crate) fn run(
    width: u32,
    height: u32,
    mut controls: Controls,
    frames: Option<usize>,
    mut draw: impl FnMut(Controls, &mut [u8]) -> Result<()>,
) -> Result<()> {
    let pitch = c_int::try_from(width.checked_mul(4).context("window width is too large")?)?;
    let size = (pitch as usize)
        .checked_mul(height as usize)
        .context("window extent is too large")?;
    ensure!(width > 0 && height > 0, "window extent must be positive");
    let video = Video::open(c_int::try_from(width)?, c_int::try_from(height)?)?;
    if frames.is_none() && matches!(video.driver(), Some(b"offscreen" | b"dummy")) {
        bail!(
            "SDL selected a headless video driver. Use --frames for a smoke test or --snapshot for a still image."
        );
    }

    eprintln!("Hair · {HELP}");
    if controls.water {
        eprintln!("Hair · {WATER_KEYS}");
    } else if rod_controls_active(&controls) {
        eprintln!("Hair · {ROD_KEYS}");
    }
    let mut fps = None;
    let mut orbit = OrbitKeys::default();
    let mut fixation = displayed_fixation(&controls);
    video.set_title(&window_title(&controls, fixation, fps));
    let mut pixels = vec![0; size];
    let mut shown = 0;
    let mut report_count = 0;
    let mut report_start = Instant::now();
    let mut previous_frame = report_start;
    while frames.is_none_or(|limit| shown < limit) {
        let now = Instant::now();
        let previous_settings = (
            controls.hair_look,
            controls.rod_tuning.wind_strength_percent,
            controls.rod_tuning.wind_heading,
        );
        if !video.poll(&mut controls, &mut orbit) {
            break;
        }
        orbit.advance(&mut controls.view_orbit, now.duration_since(previous_frame));
        previous_frame = now;

        draw(controls, &mut pixels)?;
        video.present(&pixels, pitch)?;
        shown += 1;
        report_count += 1;
        let report_interval = report_start.elapsed();
        let report_due = report_interval >= Duration::from_secs(1);
        if report_due {
            fps = Some(report_count as f32 / report_interval.as_secs_f32());
            report_start = Instant::now();
            report_count = 0;
        }
        let current_fixation = displayed_fixation(&controls);
        if report_due
            || fixation != current_fixation
            || previous_settings
                != (
                    controls.hair_look,
                    controls.rod_tuning.wind_strength_percent,
                    controls.rod_tuning.wind_heading,
                )
        {
            fixation = current_fixation;
            video.set_title(&window_title(&controls, fixation, fps));
        }
        thread::sleep(FRAME_TIME.saturating_sub(now.elapsed()));
    }
    if frames.is_some() {
        eprintln!("Presented {shown} frames with SDL2.");
    }
    Ok(())
}

fn displayed_fixation(controls: &Controls) -> Option<u16> {
    rod_controls_active(controls)
        .then(|| (10.0 * headband_fixation_percent(*controls)).round() as u16)
}

fn window_title(controls: &Controls, fixation: Option<u16>, fps: Option<f32>) -> String {
    let fps = fps.map_or_else(String::new, |fps| format!(" · {fps:.1} FPS"));
    if controls.water {
        format!(
            "Hair · Water{fps} · A/D orbit · Volume {:.2}× · Flow {:.2}× · Viscosity {:.3} · {WATER_KEYS}",
            controls.water_volume, controls.flow.speed, controls.flow.viscosity,
        )
    } else if let Some(fixation) = fixation {
        format!(
            "Hair{fps} · {} (Tab) · A/D orbit · Wind {}% toward {} · Fixation {:.1}% · 1 strength / 2 direction",
            controls.hair_look.label(),
            controls.rod_tuning.wind_strength_percent,
            controls.rod_tuning.wind_heading_label(),
            f32::from(fixation) / 10.0,
        )
    } else if let Some(motion) = controls.hair_motion {
        format!("Hair · {}{fps} · {HELP}", motion.label())
    } else {
        format!("Hair{fps} · {HELP}")
    }
}

fn rod_controls_active(controls: &Controls) -> bool {
    !controls.water && controls.hair_motion == Some(HairMotion::Rod)
}

fn rod_key(controls: &mut Controls, scancode: i32, repeat: bool, modifiers: u16) {
    if repeat || !rod_controls_active(controls) {
        return;
    }
    let backward = modifiers & MOD_SHIFT != 0;
    match scancode {
        KEY_TAB => controls.hair_look = controls.hair_look.cycle(backward),
        KEY_1 => {
            let strength = &mut controls.rod_tuning.wind_strength_percent;
            *strength = match (backward, *strength) {
                (true, 0) => WIND_STRENGTH_MAX,
                (true, value) => value.saturating_sub(WIND_STRENGTH_STEP),
                (false, WIND_STRENGTH_MAX) => 0,
                (false, value) => value
                    .saturating_add(WIND_STRENGTH_STEP)
                    .min(WIND_STRENGTH_MAX),
            };
        }
        KEY_2 => {
            let heading = &mut controls.rod_tuning.wind_heading;
            let step = if backward { WIND_HEADING_COUNT - 1 } else { 1 };
            *heading = (*heading + step) % WIND_HEADING_COUNT;
        }
        _ => (),
    }
}

const INIT_VIDEO: u32 = 0x20;
const WINDOW_CENTERED: c_int = 0x2fff_0000;
const WINDOW_RESIZABLE: u32 = 0x20;
const WINDOW_HIGH_DPI: u32 = 0x2000;
const TEXTURE_STREAMING: c_int = 1;
const RGBA32: u32 = if cfg!(target_endian = "little") {
    376_840_196 // SDL_PIXELFORMAT_ABGR8888
} else {
    373_694_468 // SDL_PIXELFORMAT_RGBA8888
};

const QUIT: u32 = 0x100;
const WINDOW_EVENT: u32 = 0x200;
const WINDOW_FOCUS_LOST: u8 = 13;
const WINDOW_CLOSE: u8 = 14;
const KEY_DOWN: u32 = 0x300;
const KEY_UP: u32 = 0x301;
const MOUSE_MOTION: u32 = 0x400;
const KEY_A: i32 = 4;
const KEY_D: i32 = 7;
const KEY_1: i32 = 30;
const KEY_2: i32 = 31;
const KEY_TAB: i32 = 43;
const KEY_I: i32 = 12;
const KEY_J: i32 = 13;
const KEY_K: i32 = 14;
const KEY_L: i32 = 15;
const KEY_O: i32 = 18;
const KEY_U: i32 = 20;
const MOD_SHIFT: u16 = 0x0003; // SDL KMOD_LSHIFT | KMOD_RSHIFT

#[derive(Clone, Copy)]
#[repr(C)]
struct WindowEvent {
    kind: u32,
    timestamp: u32,
    window_id: u32,
    event: u8,
    padding: [u8; 3],
    data: [i32; 2],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct KeyboardEvent {
    kind: u32,
    timestamp: u32,
    window_id: u32,
    state: u8,
    repeat: u8,
    padding: [u8; 2],
    scancode: i32,
    keycode: i32,
    modifiers: u16,
    unused: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct MouseMotionEvent {
    kind: u32,
    timestamp: u32,
    window_id: u32,
    which: u32,
    state: u32,
    x: i32,
    y: i32,
    xrel: i32,
    yrel: i32,
}

#[repr(C)]
union Event {
    kind: u32,
    window: WindowEvent,
    key: KeyboardEvent,
    mouse: MouseMotionEvent,
    // SDL2's public event union occupies 56 bytes on 32/64-bit targets.
    storage: [u64; 7],
}

type Handle = *mut c_void;

struct Sdl {
    _library: Library,
    init: unsafe extern "C" fn(u32) -> c_int,
    quit: unsafe extern "C" fn(),
    get_error: unsafe extern "C" fn() -> *const c_char,
    get_driver: unsafe extern "C" fn() -> *const c_char,
    create_window: unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> Handle,
    destroy_window: unsafe extern "C" fn(Handle),
    set_title: unsafe extern "C" fn(Handle, *const c_char),
    create_renderer: unsafe extern "C" fn(Handle, c_int, u32) -> Handle,
    destroy_renderer: unsafe extern "C" fn(Handle),
    set_logical_size: unsafe extern "C" fn(Handle, c_int, c_int) -> c_int,
    create_texture: unsafe extern "C" fn(Handle, u32, c_int, c_int, c_int) -> Handle,
    destroy_texture: unsafe extern "C" fn(Handle),
    update_texture: unsafe extern "C" fn(Handle, *const c_void, *const c_void, c_int) -> c_int,
    render_clear: unsafe extern "C" fn(Handle) -> c_int,
    render_copy: unsafe extern "C" fn(Handle, Handle, *const c_void, *const c_void) -> c_int,
    render_present: unsafe extern "C" fn(Handle),
    poll_event: unsafe extern "C" fn(*mut Event) -> c_int,
}

impl Sdl {
    fn load() -> Result<Self> {
        // Resolve the SDL2 runtime soname shipped by Linux distributions. This keeps
        // snapshot generation usable on machines with no SDL development package.
        let library = unsafe { Library::new("libSDL2-2.0.so.0") }
            .context("load SDL2 (the libSDL2-2.0.so.0 runtime library)")?;
        macro_rules! symbol {
            ($name:literal) => {{
                // Each inferred pointer type matches the corresponding SDL2 C ABI.
                *unsafe { library.get(concat!($name, "\0").as_bytes()) }.context($name)?
            }};
        }
        Ok(Self {
            init: symbol!("SDL_Init"),
            quit: symbol!("SDL_Quit"),
            get_error: symbol!("SDL_GetError"),
            get_driver: symbol!("SDL_GetCurrentVideoDriver"),
            create_window: symbol!("SDL_CreateWindow"),
            destroy_window: symbol!("SDL_DestroyWindow"),
            set_title: symbol!("SDL_SetWindowTitle"),
            create_renderer: symbol!("SDL_CreateRenderer"),
            destroy_renderer: symbol!("SDL_DestroyRenderer"),
            set_logical_size: symbol!("SDL_RenderSetLogicalSize"),
            create_texture: symbol!("SDL_CreateTexture"),
            destroy_texture: symbol!("SDL_DestroyTexture"),
            update_texture: symbol!("SDL_UpdateTexture"),
            render_clear: symbol!("SDL_RenderClear"),
            render_copy: symbol!("SDL_RenderCopy"),
            render_present: symbol!("SDL_RenderPresent"),
            poll_event: symbol!("SDL_PollEvent"),
            _library: library,
        })
    }

    fn error(&self, what: &str) -> anyhow::Error {
        let message = unsafe { CStr::from_ptr((self.get_error)()) }.to_string_lossy();
        anyhow::anyhow!("{what}: {message}")
    }

    fn check(&self, status: c_int, what: &str) -> Result<()> {
        if status < 0 {
            return Err(self.error(what));
        }
        Ok(())
    }
}

struct Video {
    sdl: Sdl,
    window: Handle,
    renderer: Handle,
    texture: Handle,
}

impl Video {
    fn open(width: c_int, height: c_int) -> Result<Self> {
        let sdl = Sdl::load()?;
        sdl.check(unsafe { (sdl.init)(INIT_VIDEO) }, "initialize SDL2 video")?;
        let mut video = Self {
            sdl,
            window: ptr::null_mut(),
            renderer: ptr::null_mut(),
            texture: ptr::null_mut(),
        };
        video.window = unsafe {
            (video.sdl.create_window)(
                c"Hair".as_ptr(),
                WINDOW_CENTERED,
                WINDOW_CENTERED,
                width,
                height,
                WINDOW_RESIZABLE | WINDOW_HIGH_DPI,
            )
        };
        if video.window.is_null() {
            return Err(video.sdl.error("create the Hair window"));
        }
        video.renderer = unsafe { (video.sdl.create_renderer)(video.window, -1, 0) };
        if video.renderer.is_null() {
            return Err(video.sdl.error("create the SDL2 renderer"));
        }
        video.sdl.check(
            unsafe { (video.sdl.set_logical_size)(video.renderer, width, height) },
            "set the render extent",
        )?;
        video.texture = unsafe {
            (video.sdl.create_texture)(video.renderer, RGBA32, TEXTURE_STREAMING, width, height)
        };
        if video.texture.is_null() {
            return Err(video.sdl.error("allocate the RGBA frame texture"));
        }
        Ok(video)
    }

    fn driver(&self) -> Option<&[u8]> {
        let driver = unsafe { (self.sdl.get_driver)() };
        (!driver.is_null()).then(|| unsafe { CStr::from_ptr(driver) }.to_bytes())
    }

    fn poll(&self, controls: &mut Controls, orbit: &mut OrbitKeys) -> bool {
        let mut event = Event { storage: [0; 7] };
        while unsafe { (self.sdl.poll_event)(&mut event) } == 1 {
            match unsafe { event.kind } {
                QUIT => return false,
                WINDOW_EVENT => match unsafe { event.window.event } {
                    WINDOW_CLOSE => return false,
                    WINDOW_FOCUS_LOST => *orbit = OrbitKeys::default(),
                    _ => (),
                },
                MOUSE_MOTION => {
                    let motion = unsafe { event.mouse };
                    controls.roll = (controls.roll + motion.xrel as f32 * RADIANS_PER_PIXEL)
                        .rem_euclid(std::f32::consts::TAU);
                    controls.tilt = (controls.tilt + motion.yrel as f32 * RADIANS_PER_PIXEL)
                        .clamp(-TILT_LIMIT, TILT_LIMIT);
                }
                kind @ (KEY_DOWN | KEY_UP)
                    if matches!(unsafe { event.key.scancode }, KEY_A | KEY_D) =>
                {
                    let key = unsafe { event.key };
                    if key.repeat == 0 {
                        orbit.set(key.scancode, kind == KEY_DOWN);
                    }
                }
                KEY_DOWN if controls.water => match unsafe { event.key.scancode } {
                    KEY_I => controls.water_volume = (controls.water_volume + 0.1).min(2.5),
                    KEY_K => controls.water_volume = (controls.water_volume - 0.1).max(0.15),
                    KEY_J => controls.flow.speed = (controls.flow.speed - 0.1).max(0.4),
                    KEY_L => controls.flow.speed = (controls.flow.speed + 0.1).min(1.5),
                    KEY_U => controls.flow.viscosity = (controls.flow.viscosity - 0.006).max(0.022),
                    KEY_O => controls.flow.viscosity = (controls.flow.viscosity + 0.006).min(0.12),
                    _ => (),
                },
                KEY_DOWN => {
                    let key = unsafe { event.key };
                    rod_key(controls, key.scancode, key.repeat != 0, key.modifiers);
                }
                _ => (),
            }
        }
        true
    }

    fn present(&self, pixels: &[u8], pitch: c_int) -> Result<()> {
        self.sdl.check(
            unsafe {
                (self.sdl.update_texture)(self.texture, ptr::null(), pixels.as_ptr().cast(), pitch)
            },
            "upload the RGBA frame",
        )?;
        self.sdl.check(
            unsafe { (self.sdl.render_clear)(self.renderer) },
            "clear the window",
        )?;
        self.sdl.check(
            unsafe {
                (self.sdl.render_copy)(self.renderer, self.texture, ptr::null(), ptr::null())
            },
            "draw the RGBA frame",
        )?;
        unsafe { (self.sdl.render_present)(self.renderer) };
        Ok(())
    }

    fn set_title(&self, title: &str) {
        if let Ok(title) = CString::new(title) {
            unsafe { (self.sdl.set_title)(self.window, title.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hair_look::HairLook;

    #[test]
    fn held_orbit_keys_turn_at_elapsed_rate_and_cancel_each_other() {
        let mut orbit = OrbitKeys::default();
        let mut angle = 0.0;
        let frame = Duration::from_millis(25);
        orbit.advance(&mut angle, frame);
        assert_eq!(angle, 0.0);

        orbit.set(KEY_D, true);
        for _ in 0..80 {
            orbit.advance(&mut angle, frame);
        }
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-5);

        orbit.set(KEY_A, true);
        let side = angle;
        orbit.advance(&mut angle, frame);
        assert_eq!(angle, side);

        orbit.set(KEY_D, false);
        orbit.advance(&mut angle, frame);
        assert!((angle - (side - 0.025 * ORBIT_RADIANS_PER_SECOND)).abs() < 1e-6);
        orbit.set(KEY_A, false);
        let stopped = angle;
        orbit.advance(&mut angle, frame);
        assert_eq!(angle, stopped);

        angle = 0.0;
        orbit.set(KEY_A, true);
        orbit.advance(&mut angle, frame);
        assert!((angle - (std::f32::consts::TAU - 0.025 * ORBIT_RADIANS_PER_SECOND)).abs() < 1e-6);
    }

    #[test]
    fn one_cycles_strength_and_two_cycles_heading_independently() {
        let mut controls = Controls {
            hair_motion: Some(HairMotion::Rod),
            water: false,
            ..Controls::default()
        };
        assert_eq!(controls.rod_tuning.wind_strength_percent, 0);
        assert_eq!(controls.rod_tuning.wind_heading, 0);
        for strength in (10..=100).step_by(10) {
            rod_key(&mut controls, KEY_1, false, 0);
            assert_eq!(controls.rod_tuning.wind_strength_percent, strength);
            rod_key(&mut controls, KEY_1, true, 0);
            assert_eq!(controls.rod_tuning.wind_strength_percent, strength);
        }
        rod_key(&mut controls, KEY_1, false, 0);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 0);
        rod_key(&mut controls, KEY_1, false, MOD_SHIFT);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 100);
        rod_key(&mut controls, KEY_1, false, MOD_SHIFT);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 90);
        assert_eq!(controls.rod_tuning.wind_heading, 0);

        for heading in 1..WIND_HEADING_COUNT {
            rod_key(&mut controls, KEY_2, false, 0);
            assert_eq!(controls.rod_tuning.wind_heading, heading);
            rod_key(&mut controls, KEY_2, true, 0);
            assert_eq!(controls.rod_tuning.wind_heading, heading);
        }
        rod_key(&mut controls, KEY_2, false, 0);
        assert_eq!(controls.rod_tuning.wind_heading, 0);
        rod_key(&mut controls, KEY_2, false, MOD_SHIFT);
        assert_eq!(controls.rod_tuning.wind_heading, WIND_HEADING_COUNT - 1);
        rod_key(&mut controls, KEY_2, false, MOD_SHIFT);
        assert_eq!(controls.rod_tuning.wind_heading, WIND_HEADING_COUNT - 2);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 90);
    }

    #[test]
    fn tab_cycles_hull_looks_independently_of_wind() {
        let mut controls = Controls {
            hair_look: HairLook::Locks,
            hair_motion: Some(HairMotion::Rod),
            water: false,
            ..Controls::default()
        };
        rod_key(&mut controls, KEY_1, false, 0);
        rod_key(&mut controls, KEY_2, false, 0);

        for look in [
            HairLook::Ribbon,
            HairLook::SoftMerge,
            HairLook::StrongMerge,
            HairLook::Locks,
        ] {
            rod_key(&mut controls, KEY_TAB, false, 0);
            assert_eq!(controls.hair_look, look);
            rod_key(&mut controls, KEY_TAB, true, 0);
            assert_eq!(controls.hair_look, look);
        }

        rod_key(&mut controls, KEY_TAB, false, MOD_SHIFT);
        assert_eq!(controls.hair_look, HairLook::StrongMerge);
        rod_key(&mut controls, KEY_TAB, false, MOD_SHIFT);
        assert_eq!(controls.hair_look, HairLook::SoftMerge);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 10);
        assert_eq!(controls.rod_tuning.wind_heading, 1);

        let title = window_title(&controls, displayed_fixation(&controls), None);
        assert!(title.contains(&format!("{} (Tab)", controls.hair_look.label())));
    }

    #[test]
    fn title_reports_wind_and_live_fixation() {
        let mut controls = Controls {
            hair_motion: Some(HairMotion::Rod),
            water: false,
            ..Controls::default()
        };
        rod_key(&mut controls, KEY_1, false, 0);
        let title = window_title(&controls, displayed_fixation(&controls), None);
        assert!(title.contains("A/D orbit · Wind 10% toward +X · Fixation 50.0%"));

        rod_key(&mut controls, KEY_2, false, 0);
        controls.roll = std::f32::consts::PI;
        let title = window_title(&controls, displayed_fixation(&controls), None);
        let heading = controls.rod_tuning.wind_heading_label();
        assert!(title.contains(&format!("Wind 10% toward {heading} · Fixation 0.0%")));
    }

    #[test]
    fn rod_tuning_ignores_other_keys_and_modes() {
        let mut controls = Controls {
            hair_motion: Some(HairMotion::Rod),
            water: false,
            ..Controls::default()
        };
        let initial_look = controls.hair_look;
        for scancode in [KEY_A, KEY_D, KEY_I, KEY_J, KEY_K, KEY_L, 32] {
            rod_key(&mut controls, scancode, false, 0);
        }
        assert_eq!(controls.hair_look, initial_look);
        assert_eq!(controls.rod_tuning.wind_heading, 0);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 0);

        controls.water = true;
        rod_key(&mut controls, KEY_1, false, 0);
        rod_key(&mut controls, KEY_2, false, 0);
        rod_key(&mut controls, KEY_TAB, false, 0);
        assert_eq!(controls.hair_look, initial_look);
        assert_eq!(controls.rod_tuning.wind_heading, 0);
        assert_eq!(controls.rod_tuning.wind_strength_percent, 0);
        assert_eq!(displayed_fixation(&controls), None);
        controls.water = false;
        for motion in [Some(HairMotion::Spring), Some(HairMotion::Rope), None] {
            controls.hair_motion = motion;
            rod_key(&mut controls, KEY_1, false, 0);
            rod_key(&mut controls, KEY_2, false, 0);
            rod_key(&mut controls, KEY_TAB, false, 0);
            assert_eq!(controls.hair_look, initial_look);
            assert_eq!(controls.rod_tuning.wind_heading, 0);
            assert_eq!(controls.rod_tuning.wind_strength_percent, 0);
            assert_eq!(displayed_fixation(&controls), None);
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            if !self.texture.is_null() {
                (self.sdl.destroy_texture)(self.texture);
            }
            if !self.renderer.is_null() {
                (self.sdl.destroy_renderer)(self.renderer);
            }
            if !self.window.is_null() {
                (self.sdl.destroy_window)(self.window);
            }
            (self.sdl.quit)();
        }
    }
}
