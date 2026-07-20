#![no_std]

extern crate alloc;
pub extern crate alloc as alloc_crate;
#[cfg(feature = "tokio-runtime")]
extern crate std;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
#[cfg(feature = "tokio-runtime")]
pub use tokio;
pub use v::calculator_base;
pub use v::collections;
pub use v::env;
pub use v::vaudio as audio;
pub use v::vclock as clock;
pub use v::vfs_async as async_fs;
pub use v::vinput as hid;
pub use v::vmail;
pub use v::vnet;
pub use v::vpci as pci;
pub use v::vprint2d as print2d;
pub use v::vprinter as printers;
pub use v::vrapl as rapl;
pub use v::vshell;
pub use v::vsys;
pub use v::vsystem_services as system_services;
pub use v::vthermal as thermal;

/// Keyboard events translated by the kernel's shared HID input broker.
///
/// `hid` exposes device-oriented input state. This small facade exposes the
/// focus-independent key/text stream used by interactive Blueprint windows.
pub mod input {
    pub const KEYBOARD_OUTPUT_KIND_TEXT: u8 = 1;
    pub const KEYBOARD_OUTPUT_KIND_KEY: u8 = 2;

    pub const KEYBOARD_KEY_ENTER: u16 = 3;
    pub const KEYBOARD_KEY_ESCAPE: u16 = 4;
    pub const KEYBOARD_KEY_SPACE: u16 = 5;
    pub const KEYBOARD_KEY_ARROW_UP: u16 = 12;
    pub const KEYBOARD_KEY_ARROW_DOWN: u16 = 13;
    pub const KEYBOARD_KEY_ARROW_LEFT: u16 = 14;
    pub const KEYBOARD_KEY_ARROW_RIGHT: u16 = 15;

    pub use v::bp_abi::TrueosKeyboardOutputEvent;

    #[inline]
    pub fn pop_keyboard_output() -> Option<TrueosKeyboardOutputEvent> {
        let mut event = TrueosKeyboardOutputEvent::default();
        let result =
            unsafe { v::bp_abi::trueos_cabi_input_pop_keyboard_output(&mut event as *mut _) };
        (result == 0).then_some(event)
    }
}

pub mod ui4_solara_text;

pub mod ui4_scene;

#[cfg(feature = "lifecycle-net")]
pub mod lifecycle;

#[cfg(feature = "gridpaper")]
pub mod gridpaper;

pub mod platform {
    pub use alloc::borrow::{Cow, ToOwned};
    pub use alloc::boxed::Box;
    pub use alloc::format;
    pub use alloc::string::{String, ToString};
    pub use alloc::sync::Arc;
    pub use alloc::vec;
    pub use alloc::vec::Vec;
    pub use core::future;

    pub use v::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    pub use v::vsys::{poll_once, sleep_ms, write_stream};

    #[cfg(feature = "tokio-runtime")]
    pub fn spawn_blocking<F>(f: F) -> Result<(), ()>
    where
        F: FnOnce() + Send + 'static,
    {
        tokio::task::spawn_blocking(f);
        Ok(())
    }

    #[cfg(feature = "tokio-runtime")]
    pub mod io {
        pub use tokio::io::{Error, ErrorKind, Result, SeekFrom};
    }

    #[cfg(feature = "tokio-runtime")]
    pub mod path {
        pub use std::path::{Component, Components, Path, PathBuf};
    }

    #[cfg(feature = "tokio-runtime")]
    pub mod thread {
        pub use std::thread::{Thread, ThreadId, current};

        #[inline]
        pub fn yield_now() {
            v::vsys::poll_once();
        }
    }
}

pub mod logl {
    use alloc::string::{String, ToString};
    use core::fmt::Write as _;

    pub mod level {
        pub const ERROR: u8 = 1;
        pub const WARN: u8 = 2;
        pub const INFO: u8 = 3;
        pub const DEBUG: u8 = 4;
        pub const TRACE: u8 = 5;
    }

    pub fn log(level: u8, message: impl IntoLogMessage) {
        let stream = if level <= level::ERROR { 2 } else { 1 };
        let line = message.into_log_message();
        v::vsys::write_stream(stream, line.as_bytes());
        if !line.ends_with('\n') {
            v::vsys::write_stream(stream, b"\n");
        }
    }

    pub fn log_record(level: u8, target: &str, message: impl IntoLogMessage) -> i32 {
        let message = message.into_log_message();
        v::vsys::log_record(u32::from(level), target, message.as_str())
    }

    pub trait IntoLogMessage {
        fn into_log_message(self) -> String;
    }

    impl IntoLogMessage for &str {
        fn into_log_message(self) -> String {
            self.to_string()
        }
    }

    impl IntoLogMessage for core::fmt::Arguments<'_> {
        fn into_log_message(self) -> String {
            let mut out = String::new();
            let _ = out.write_fmt(self);
            out
        }
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod runtime {
    pub use tokio::runtime::{Builder, Handle, Runtime};

    pub fn current_thread() -> Builder {
        let mut builder = tokio::runtime::Builder::new_current_thread();
        builder.enable_time();
        builder
    }

    #[cfg(feature = "tokio-net-probe")]
    pub fn current_thread_net() -> Builder {
        let mut builder = current_thread();
        builder.enable_io();
        builder
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod task {
    pub use tokio::spawn;
    pub use tokio::task::{JoinError, JoinHandle, JoinSet, LocalSet, yield_now};
}

#[cfg(feature = "tokio-runtime")]
pub mod sync {
    pub use tokio::sync::{
        Barrier, Mutex, Notify, RwLock, Semaphore, broadcast, mpsc, oneshot, watch,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod time {
    pub use tokio::time::{Duration, Instant, Interval, Sleep, interval, sleep, timeout};
}

pub mod rng {
    #[inline]
    pub fn fill(bytes: &mut [u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut offset = 0usize;
        while offset < bytes.len() {
            let mut word = u32();
            let chunk = core::cmp::min(core::mem::size_of::<u32>(), bytes.len() - offset);
            bytes[offset..offset + chunk].copy_from_slice(&word.to_ne_bytes()[..chunk]);
            offset += chunk;
        }
    }

    #[inline]
    pub fn u8() -> u8 {
        u32() as u8
    }

    #[inline]
    pub fn i8() -> i8 {
        u8() as i8
    }

    #[inline]
    pub fn u16() -> u16 {
        u32() as u16
    }

    #[inline]
    pub fn i16() -> i16 {
        u16() as i16
    }

    #[inline]
    pub fn u32() -> u32 {
        let mut word = 0u32;
        unsafe { v::vcabi::sys_rand(&mut word, 1) };
        word
    }

    #[inline]
    pub fn i32() -> i32 {
        u32() as i32
    }

    #[inline]
    pub fn u64() -> u64 {
        let lo = u32() as u64;
        let hi = u32() as u64;
        lo | (hi << 32)
    }

    #[inline]
    pub fn i64() -> i64 {
        u64() as i64
    }

    #[inline]
    pub fn u128() -> u128 {
        let lo = u64() as u128;
        let hi = u64() as u128;
        lo | (hi << 64)
    }

    #[inline]
    pub fn i128() -> i128 {
        u128() as i128
    }

    #[inline]
    pub fn usize() -> usize {
        if usize::BITS <= 32 {
            u32() as usize
        } else {
            u64() as usize
        }
    }

    #[inline]
    pub fn isize() -> isize {
        usize() as isize
    }

    #[inline]
    pub fn boolean() -> bool {
        (u32() & 1) != 0
    }

    #[inline]
    pub fn f32() -> f32 {
        const SCALE: f32 = 1.0 / ((1u64 << 24) as f32);
        ((u32() >> 8) as f32) * SCALE
    }

    #[inline]
    pub fn f64() -> f64 {
        const SCALE: f64 = 1.0 / ((1u64 << 53) as f64);
        ((u64() >> 11) as f64) * SCALE
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod net {
    use core::fmt;

    #[cfg(feature = "tokio-net-probe")]
    pub use tokio::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket, lookup_host};

    #[cfg(feature = "tokio-net-probe")]
    pub mod mio {
        pub use mio::{Events, Interest, Poll, Registry, Token, Waker};
        pub use mio::{event, net};
    }

    #[cfg(feature = "tokio-net-probe")]
    pub mod socket2 {
        pub use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    }

    pub const DEFAULT_TUN_MTU: u16 = 1500;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum TunError {
        BadArgument,
        WouldBlock,
        MessageTooLarge,
        NetworkUnreachable,
        NotConnected,
        Io,
        Unknown(isize),
    }

    impl fmt::Display for TunError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                TunError::BadArgument => f.write_str("bad TUN argument"),
                TunError::WouldBlock => f.write_str("TUN would block"),
                TunError::MessageTooLarge => f.write_str("TUN packet is too large"),
                TunError::NetworkUnreachable => f.write_str("TUN network is unreachable"),
                TunError::NotConnected => f.write_str("TUN is not connected"),
                TunError::Io => f.write_str("TUN I/O error"),
                TunError::Unknown(code) => write!(f, "TUN error {}", code),
            }
        }
    }

    impl TunError {
        fn from_rc(rc: isize) -> Self {
            match -rc {
                11 => Self::WouldBlock,
                22 => Self::BadArgument,
                32 | 107 => Self::NotConnected,
                90 => Self::MessageTooLarge,
                101 => Self::NetworkUnreachable,
                5 => Self::Io,
                _ => Self::Unknown(rc),
            }
        }
    }

    #[derive(Debug)]
    pub struct Tun {
        id: u32,
    }

    impl Tun {
        pub fn open(
            ipv4: [u8; 4],
            ipv4_prefix_len: u8,
            ipv6: [u8; 16],
            ipv6_prefix_len: u8,
            mtu: u16,
        ) -> Result<Self, TunError> {
            let rc = unsafe {
                v::vcabi::trueos_cabi_tun_open(
                    u32::from_be_bytes(ipv4),
                    ipv4_prefix_len as u32,
                    ipv6.as_ptr(),
                    ipv6_prefix_len as u32,
                    mtu as u32,
                )
            };
            if rc > 0 {
                Ok(Self { id: rc as u32 })
            } else {
                Err(TunError::from_rc(rc as isize))
            }
        }

        pub fn recv(&self, out: &mut [u8]) -> Result<usize, TunError> {
            let rc =
                unsafe { v::vcabi::trueos_cabi_tun_recv(self.id, out.as_mut_ptr(), out.len()) };
            if rc >= 0 {
                Ok(rc as usize)
            } else {
                Err(TunError::from_rc(rc))
            }
        }

        pub fn send(&self, packet: &[u8]) -> Result<usize, TunError> {
            let rc =
                unsafe { v::vcabi::trueos_cabi_tun_send(self.id, packet.as_ptr(), packet.len()) };
            if rc >= 0 {
                Ok(rc as usize)
            } else {
                Err(TunError::from_rc(rc))
            }
        }
    }

    impl Drop for Tun {
        fn drop(&mut self) {
            let _ = unsafe { v::vcabi::trueos_cabi_tun_close(self.id) };
        }
    }
}

#[cfg(feature = "tokio-runtime")]
pub mod io {
    pub use tokio::io::{
        AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, Stderr, Stdin, Stdout, duplex,
        stderr, stdin, stdout,
    };
}

#[cfg(feature = "tokio-runtime")]
pub mod fs {
    pub use tokio::fs::{
        File, OpenOptions, canonicalize, copy, create_dir, create_dir_all, metadata, read,
        read_to_string, remove_dir_all, remove_file, rename, try_exists, write,
    };
}

#[cfg(feature = "ui3")]
mod ui3_core {
    extern crate alloc;

    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Rect {
        pub x: i32,
        pub y: i32,
        pub width: u32,
        pub height: u32,
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct CursorEvent {
        pub slot_id: u32,
        pub buttons_down: u32,
        pub wheel: i16,
        pub x: f32,
        pub y: f32,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct WindowId(u32);

    impl WindowId {
        #[inline]
        pub const fn new(raw: u32) -> Option<Self> {
            if raw == 0 { None } else { Some(Self(raw)) }
        }

        #[inline]
        pub const fn raw(self) -> u32 {
            self.0
        }

        #[inline]
        pub fn request_repaint(self) -> bool {
            unsafe { trueos_cabi_ui3_frame_request_repaint(self.0) == 0 }
        }

        #[inline]
        pub fn close(self) -> bool {
            unsafe { trueos_cabi_ui3_frame_close(self.0) == 0 }
        }

        #[inline]
        pub fn begin_move(self) -> bool {
            let _ = self;
            false
        }

        #[inline]
        pub fn set_position(self, x: i32, y: i32) -> bool {
            unsafe { trueos_cabi_ui3_frame_set_position(self.0, x, y) == 0 }
        }

        #[inline]
        pub fn set_size(self, width: u32, height: u32) -> bool {
            unsafe { trueos_cabi_ui3_frame_set_size(self.0, width, height) == 0 }
        }

        pub fn take_cursor_events(self, max_events: u32) -> Vec<CursorEvent> {
            static NEXT_CURSOR_SEQ: AtomicU64 = AtomicU64::new(0);

            let cap = max_events.min(256);
            if cap == 0 {
                return Vec::new();
            }

            let read_seq = NEXT_CURSOR_SEQ.load(Ordering::Relaxed);
            let mut raw = Vec::with_capacity(cap as usize);
            raw.resize_with(cap as usize, v::bp_abi::TrueosHidCursorEvent::default);
            let mut next_seq = read_seq;
            let mut dropped = 0u32;
            let got = unsafe {
                v::bp_abi::trueos_cabi_input_read_cursor_events_since(
                    read_seq,
                    raw.as_mut_ptr(),
                    cap,
                    &mut next_seq,
                    &mut dropped,
                )
            };
            NEXT_CURSOR_SEQ.store(next_seq, Ordering::Relaxed);
            raw.truncate(got.min(cap) as usize);
            raw.into_iter()
                .map(|event| CursorEvent {
                    slot_id: event.slot_id,
                    buttons_down: event.buttons_down,
                    wheel: event.wheel,
                    x: event.x as f32,
                    y: event.y as f32,
                })
                .collect()
        }
    }

    static CURRENT_FRAME_ID: AtomicU32 = AtomicU32::new(0);

    #[derive(Debug)]
    pub struct SurfaceWindow {
        id: WindowId,
        tex_id: u32,
        close_on_drop: bool,
    }

    impl SurfaceWindow {
        pub fn create(title: &str, rect: Rect, tex_id: u32) -> Option<Self> {
            let _ = title;
            let raw = unsafe {
                trueos_cabi_ui3_frame_create(rect.x, rect.y, rect.width, rect.height, tex_id)
            };
            WindowId::new(raw).map(|id| {
                CURRENT_FRAME_ID.store(id.raw(), Ordering::Relaxed);
                Self {
                    id,
                    tex_id,
                    close_on_drop: true,
                }
            })
        }

        #[inline]
        pub const fn id(&self) -> WindowId {
            self.id
        }

        #[inline]
        pub const fn tex_id(&self) -> u32 {
            self.tex_id
        }

        pub fn leak(mut self) -> WindowId {
            self.close_on_drop = false;
            CURRENT_FRAME_ID.store(self.id.raw(), Ordering::Relaxed);
            self.id
        }
    }

    impl Drop for SurfaceWindow {
        fn drop(&mut self) {
            if self.close_on_drop {
                let _ = self.id.close();
            }
        }
    }

    pub mod gfx {
        use core::sync::atomic::Ordering;

        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct SolidRect {
            pub x: f32,
            pub y: f32,
            pub w: f32,
            pub h: f32,
            pub color: [u8; 4],
        }

        impl SolidRect {
            #[inline]
            pub const fn new(x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) -> Self {
                Self { x, y, w, h, color }
            }
        }

        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct SpriteCorner {
            pub x: f32,
            pub y: f32,
            pub u: f32,
            pub v: f32,
        }

        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct SpriteQuad {
            pub c0: SpriteCorner,
            pub c1: SpriteCorner,
            pub c2: SpriteCorner,
            pub c3: SpriteCorner,
            pub color: [u8; 4],
        }

        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default)]
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

        #[inline]
        pub fn upload_texture_rgba_image_now(
            tex_id: u32,
            width: u32,
            height: u32,
            rgba: &[u8],
        ) -> bool {
            unsafe {
                super::trueos_cabi_gfx_upload_texture_rgba_image(
                    tex_id,
                    width,
                    height,
                    rgba.as_ptr(),
                    rgba.len(),
                ) == 0
            }
        }

        #[inline]
        pub fn upload_skybox_rgb565_now(
            skybox_id: u32,
            width: u32,
            height: u32,
            rgb565: &[u8],
        ) -> bool {
            unsafe {
                super::trueos_cabi_gfx_upload_skybox_rgb565(
                    skybox_id,
                    width,
                    height,
                    rgb565.as_ptr(),
                    rgb565.len(),
                ) == 0
            }
        }

        #[inline]
        pub fn texture_status(tex_id: u32) -> i32 {
            unsafe { super::trueos_cabi_gfx_texture_status(tex_id) }
        }

        #[inline]
        pub fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
            let mut width = 0u32;
            let mut height = 0u32;
            let rc = unsafe {
                super::trueos_cabi_gfx_texture_dimensions(tex_id, &mut width, &mut height)
            };
            (rc == 0).then_some((width, height))
        }

        #[inline]
        pub fn begin_frame_preserve(clear_rgb: u32) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe { super::trueos_cabi_ui3_frame_begin(frame_id, clear_rgb, 1, 1) }
        }

        #[inline]
        pub fn begin_frame_no_present(clear_rgb: u32) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe { super::trueos_cabi_ui3_frame_begin(frame_id, clear_rgb, 0, 0) }
        }

        #[inline]
        pub fn end_frame() -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe { super::trueos_cabi_ui3_frame_end(frame_id) }
        }

        #[inline]
        pub fn set_render_target(tex_id: u32) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe { super::trueos_cabi_ui3_frame_set_render_target(frame_id, tex_id) }
        }

        #[inline]
        pub fn draw_solid_batch_no_present(rects: &[SolidRect]) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe {
                super::trueos_cabi_ui3_frame_draw_solid_batch(
                    frame_id,
                    rects.as_ptr() as *const u8,
                    core::mem::size_of_val(rects),
                )
            }
        }

        #[inline]
        pub fn draw_sprite_batch_no_present(tex_id: u32, quads: &[u8]) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe {
                super::trueos_cabi_ui3_frame_draw_sprite_batch(
                    frame_id,
                    tex_id,
                    quads.as_ptr(),
                    quads.len(),
                )
            }
        }

        #[inline]
        pub fn render_skybox_rgb565_no_present(skybox_id: u32, params: &SkyboxRenderParams) -> i32 {
            let frame_id = super::CURRENT_FRAME_ID.load(Ordering::Relaxed);
            unsafe {
                super::trueos_cabi_ui3_frame_render_skybox_rgb565(
                    frame_id,
                    skybox_id,
                    (params as *const SkyboxRenderParams).cast::<u8>(),
                    core::mem::size_of::<SkyboxRenderParams>(),
                )
            }
        }

        #[inline]
        pub fn set_blend_raw(
            enabled: u32,
            src_rgb: u32,
            dst_rgb: u32,
            src_alpha: u32,
            dst_alpha: u32,
            equation_rgb: u32,
            equation_alpha: u32,
        ) -> i32 {
            let _ = (
                enabled,
                src_rgb,
                dst_rgb,
                src_alpha,
                dst_alpha,
                equation_rgb,
                equation_alpha,
            );
            0
        }

        #[inline]
        pub fn set_sampler_raw(wrap_s: u32, wrap_t: u32, min_filter: u32, mag_filter: u32) -> i32 {
            let _ = (wrap_s, wrap_t, min_filter, mag_filter);
            0
        }

        #[inline]
        pub fn set_scissor(x: u32, y: u32, width: u32, height: u32) -> i32 {
            let _ = (x, y, width, height);
            0
        }

        #[inline]
        pub fn clear_scissor() -> i32 {
            0
        }
    }

    unsafe extern "C" {
        fn trueos_cabi_gfx_texture_dimensions(
            tex_id: u32,
            out_width: *mut u32,
            out_height: *mut u32,
        ) -> i32;
        fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
        fn trueos_cabi_gfx_upload_texture_rgba_image(
            tex_id: u32,
            width: u32,
            height: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_skybox_rgb565(
            skybox_id: u32,
            width: u32,
            height: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_ui3_frame_create(
            x: i32,
            y: i32,
            width: u32,
            height: u32,
            tex_id: u32,
        ) -> u32;
        fn trueos_cabi_ui3_frame_close(frame_id: u32) -> i32;
        fn trueos_cabi_ui3_frame_request_repaint(frame_id: u32) -> i32;
        fn trueos_cabi_ui3_frame_set_position(frame_id: u32, x: i32, y: i32) -> i32;
        fn trueos_cabi_ui3_frame_set_size(frame_id: u32, width: u32, height: u32) -> i32;
        fn trueos_cabi_ui3_frame_begin(
            frame_id: u32,
            clear_rgb: u32,
            preserve_contents: u32,
            allow_present: u32,
        ) -> i32;
        fn trueos_cabi_ui3_frame_end(frame_id: u32) -> i32;
        fn trueos_cabi_ui3_frame_set_render_target(frame_id: u32, tex_id: u32) -> i32;
        fn trueos_cabi_ui3_frame_draw_solid_batch(
            frame_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_ui3_frame_draw_sprite_batch(
            frame_id: u32,
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_ui3_frame_render_skybox_rgb565(
            frame_id: u32,
            skybox_id: u32,
            params_ptr: *const u8,
            params_len: usize,
        ) -> i32;
    }
}

#[cfg(feature = "ui3")]
pub mod ui3 {
    pub mod frame {
        pub type FrameBounds = crate::ui3_core::Rect;
        pub type FrameId = crate::ui3_core::WindowId;
        pub type Frame = crate::ui3_core::SurfaceWindow;
        pub type CursorEvent = crate::ui3_core::CursorEvent;
    }

    pub mod gfx {
        pub use crate::ui3_core::gfx::*;
    }

    pub use frame::{CursorEvent, Frame, FrameBounds, FrameId};
}

#[cfg(feature = "tokio-runtime")]
pub mod t {
    pub use crate::fs;
    pub use crate::io;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::runtime;
    pub use crate::sync;
    pub use crate::task;
    pub use crate::time;
    pub use crate::tokio;
}

pub struct TrueosAllocator;

#[cfg(feature = "default-global-allocator")]
#[global_allocator]
static DEFAULT_GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            return unsafe { alloc_aligned(layout) };
        }
        unsafe { v::vcabi::trueos_cabi_alloc(layout.size().max(1)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        unsafe { v::vcabi::trueos_cabi_free(ptr) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() > core::mem::align_of::<usize>() {
            let ptr = unsafe { alloc_aligned(layout) };
            if !ptr.is_null() {
                unsafe { core::ptr::write_bytes(ptr, 0, layout.size().max(1)) };
            }
            return ptr;
        }
        unsafe { v::vcabi::trueos_cabi_calloc(1, layout.size().max(1)) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ptr.is_null() {
            return unsafe { self.alloc(layout) };
        }
        if layout.align() > core::mem::align_of::<usize>() {
            let Some(new_layout) = Layout::from_size_align(new_size.max(1), layout.align()).ok()
            else {
                return null_mut();
            };
            let new_ptr = unsafe { alloc_aligned(new_layout) };
            if !new_ptr.is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        ptr,
                        new_ptr,
                        core::cmp::min(layout.size(), new_size),
                    );
                    v::vcabi::trueos_cabi_free(ptr);
                }
            }
            return new_ptr;
        }
        unsafe { v::vcabi::trueos_cabi_realloc(ptr, new_size.max(1)) }
    }
}

unsafe fn alloc_aligned(layout: Layout) -> *mut u8 {
    unsafe { v::vcabi::sys_alloc_aligned(layout.size().max(1), layout.align()) }
}

pub fn panic_abort(message: &str) -> ! {
    v::vsys::write_err(message.as_bytes());
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    panic_abort("blueprint abort\n")
}

#[cfg(feature = "default-panic-handler")]
#[panic_handler]
fn default_panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    panic_abort("blueprint panic\n")
}

pub mod prelude {
    pub use crate::TrueosAllocator;
    pub use crate::logl;
    #[cfg(feature = "tokio-net-probe")]
    pub use crate::net;
    pub use crate::platform;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::t;
    #[cfg(feature = "tokio-runtime")]
    pub use crate::{fs, io, runtime, sync, task, time, tokio};
}
