use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr, thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use libloading::Library;

use crate::Controls;

const TARGET_HZ: u64 = 45;
const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / TARGET_HZ);
const HELP: &str = "Move the mouse left or right to spin";
const RADIANS_PER_PIXEL: f32 = 0.008;

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
    let mut pixels = vec![0; size];
    let mut shown = 0;
    let mut report_count = 0;
    let mut report_start = Instant::now();
    while frames.is_none_or(|limit| shown < limit) {
        let now = Instant::now();
        if !video.poll(&mut controls.yaw) {
            break;
        }

        draw(controls, &mut pixels)?;
        video.present(&pixels, pitch)?;
        shown += 1;
        report_count += 1;
        let report_interval = report_start.elapsed();
        if report_interval >= Duration::from_secs(1) {
            let fps = report_count as f32 / report_interval.as_secs_f32();
            video.set_title(&format!("Hair · {fps:.1} FPS · {HELP}"));
            report_start = Instant::now();
            report_count = 0;
        }
        thread::sleep(FRAME_TIME.saturating_sub(now.elapsed()));
    }
    if frames.is_some() {
        eprintln!("Presented {shown} frames with SDL2.");
    }
    Ok(())
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
const WINDOW_CLOSE: u8 = 14;
const MOUSE_MOTION: u32 = 0x400;

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

    fn poll(&self, yaw: &mut f32) -> bool {
        let mut event = Event { storage: [0; 7] };
        while unsafe { (self.sdl.poll_event)(&mut event) } == 1 {
            match unsafe { event.kind } {
                QUIT => return false,
                WINDOW_EVENT if unsafe { event.window.event } == WINDOW_CLOSE => return false,
                MOUSE_MOTION => {
                    let horizontal = unsafe { event.mouse.xrel };
                    *yaw = (*yaw + horizontal as f32 * RADIANS_PER_PIXEL)
                        .rem_euclid(std::f32::consts::TAU);
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
