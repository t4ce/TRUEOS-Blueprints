// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
use core3::io::Cursor;
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, output_dimensions, rgba};
use trueos::{async_fs, image_source, input, vmedia, vsys};

const MAX_SOURCE_PIXELS: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 32;

struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
enum Alignment {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

struct OpenFrame {
    frame: Frame,
    view: View,
    image: Image,
}

#[derive(Clone, Copy)]
struct View {
    viewport_width: u32,
    viewport_height: u32,
    native_viewport_width: u32,
    native_viewport_height: u32,
    image_width: u32,
    image_height: u32,
    offset_x: f32,
    offset_y: f32,
    letterbox: bool,
}

impl View {
    fn new(
        viewport_width: u32,
        viewport_height: u32,
        image_width: u32,
        image_height: u32,
        alignment: Alignment,
    ) -> Self {
        let overflow_x = image_width.saturating_sub(viewport_width) as f32;
        let overflow_y = image_height.saturating_sub(viewport_height) as f32;
        let (offset_x, offset_y) = match alignment {
            Alignment::Center => (-overflow_x * 0.5, -overflow_y * 0.5),
            Alignment::TopLeft => (0.0, 0.0),
            Alignment::TopRight => (-overflow_x, 0.0),
            Alignment::BottomLeft => (0.0, -overflow_y),
            Alignment::BottomRight => (-overflow_x, -overflow_y),
        };
        let mut view = Self {
            viewport_width,
            viewport_height,
            native_viewport_width: viewport_width,
            native_viewport_height: viewport_height,
            image_width,
            image_height,
            offset_x,
            offset_y,
            letterbox: false,
        };
        view.clamp_offsets();
        view
    }

    fn pan(&mut self, dx: i32, dy: i32) {
        if self.letterbox {
            return;
        }
        self.offset_x += dx as f32;
        self.offset_y += dy as f32;
        self.clamp_offsets();
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.viewport_width = width;
        self.viewport_height = height;
        self.letterbox =
            width != self.native_viewport_width || height != self.native_viewport_height;
        self.clamp_offsets();
    }

    fn clamp_offsets(&mut self) {
        self.offset_x = clamp_axis(
            self.offset_x,
            self.viewport_width as f32,
            self.image_width as f32,
        );
        self.offset_y = clamp_axis(
            self.offset_y,
            self.viewport_height as f32,
            self.image_height as f32,
        );
    }
}

fn clamp_axis(offset: f32, viewport: f32, content: f32) -> f32 {
    if content <= viewport {
        (viewport - content) * 0.5
    } else {
        offset.clamp(viewport - content, 0.0)
    }
}

fn main() {
    let mut frames = Vec::new();
    let args: Vec<String> = trueos::env::args().skip(1).collect();
    if !args.is_empty() {
        let mut command = String::from("show ");
        command.push_str(args.join(" ").as_str());
        run_line(command.as_str(), &mut frames);
    }
    terminal_enter();
    terminal_write(b"img: interactive UI4 media viewer (up to 32 frames)\r\n");
    terminal_write(b"img: `show PATH [center|top-left|top-right|bottom-left|bottom-right] [hit|nohit]`; `list`; `close all`; `exit`\r\nimg> ");
    let mut command = Vec::new();

    loop {
        service_frames(&mut frames);
        if service_terminal(&mut command, &mut frames) {
            break;
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
    let _ = trueos::vshell::shutdown_current_blueprint("img viewer exited");
}

fn terminal_enter() {
    let size = trueos::vshell::konsole_size()
        .unwrap_or(trueos::vshell::KonsoleSize { cols: 80, rows: 24 });
    let _ = trueos::vshell::konsole_begin_frame(
        size.cols,
        size.rows,
        trueos::vshell::KONSOLE_FRAME_TERMINAL_HANDOFF,
    );
}

fn terminal_write(bytes: &[u8]) {
    let _ = trueos::vshell::attached_write(bytes);
}

fn service_terminal(command: &mut Vec<u8>, frames: &mut Vec<OpenFrame>) -> bool {
    let mut bytes = [0u8; 512];
    let len = trueos::vshell::attached_read_available(&mut bytes);
    for byte in &bytes[..len] {
        match *byte {
            3 => {
                terminal_write(b"^C\r\nimg> ");
                command.clear();
            }
            b'\r' | b'\n' => {
                terminal_write(b"\r\n");
                let line = core::mem::take(command);
                let line = String::from_utf8(line).unwrap_or_default();
                match line.trim() {
                    "" => {}
                    "help" => terminal_write(
                        b"show PATH [alignment] [hit|nohit], list, close all, exit\r\n",
                    ),
                    "list" => terminal_write(
                        format!("img: {} of {} frames open\r\n", frames.len(), MAX_FRAMES)
                            .as_bytes(),
                    ),
                    "close all" | "clear" => {
                        frames.clear();
                        terminal_write(b"img: all frames closed\r\n");
                    }
                    "exit" | "quit" => {
                        terminal_write(b"img: leaving interactive viewer\r\n");
                        trueos::vshell::leave_terminal_handoff();
                        return true;
                    }
                    line if line.starts_with("show ") => run_line(line, frames),
                    path => {
                        let line = format!("show {path}");
                        run_line(line.as_str(), frames);
                    }
                }
                terminal_write(b"img> ");
            }
            8 | 127 if !command.is_empty() => {
                command.pop();
                terminal_write(b"\x08 \x08");
            }
            byte if byte >= 0x20 => {
                command.push(byte);
                terminal_write(&[byte]);
            }
            _ => {}
        }
    }
    false
}

fn run_line(line: &str, frames: &mut Vec<OpenFrame>) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }
    let Some(command) = line.strip_prefix("show ") else {
        logl::log(level::WARN, format_args!("img: ignored command={line}"));
        return;
    };
    let mut words = command.split_ascii_whitespace();
    let Some(path) = words.next() else {
        logl::log(level::WARN, format_args!("img: show requires a source"));
        return;
    };
    let mut alignment = Alignment::Center;
    let mut hit_testable = true;
    for option in words {
        match option {
            "center" => alignment = Alignment::Center,
            "top-left" => alignment = Alignment::TopLeft,
            "top-right" => alignment = Alignment::TopRight,
            "bottom-left" => alignment = Alignment::BottomLeft,
            "bottom-right" => alignment = Alignment::BottomRight,
            "nohit" => hit_testable = false,
            "hit" => hit_testable = true,
            _ => {
                logl::log(
                    level::WARN,
                    format_args!("img: unknown show option={option}"),
                );
                return;
            }
        }
    }
    if frames.len() >= MAX_FRAMES {
        logl::log(
            level::WARN,
            format_args!("img: frame cap={} source={path}", MAX_FRAMES),
        );
        return;
    }
    let image = match load_image(path.trim()) {
        Ok(image) => image,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("img: show source={path} error={error}"),
            );
            return;
        }
    };
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let viewport_width = image.width.min(output_width).max(1);
    let viewport_height = image.height.min(output_height).max(1);
    let (x, y) = aligned_position(
        output_width,
        output_height,
        viewport_width,
        viewport_height,
        alignment,
    );
    let mut frame = match Frame::open_immutable(x, y, viewport_width, viewport_height) {
        Ok(frame) => frame,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!(
                    "img: viewport rejected source={path} image={}x{} viewport={}x{} error={error:?}",
                    image.width, image.height, viewport_width, viewport_height
                ),
            );
            return;
        }
    };
    if let Err(error) = frame.set_hit_testable(hit_testable) {
        logl::log(
            level::ERROR,
            format_args!("img: hit-test source={path} error={error:?}"),
        );
        return;
    }
    let view = View::new(
        viewport_width,
        viewport_height,
        image.width,
        image.height,
        alignment,
    );
    if let Err(error) = present(&mut frame, view, &image) {
        logl::log(
            level::ERROR,
            format_args!("img: present source={path} error={error:?}"),
        );
        return;
    }
    logl::log(
        level::INFO,
        format_args!(
            "img: show source={path} size={}x{} viewport={}x{} window={} align={alignment:?} hit={} native=1 frames={}/{}",
            image.width,
            image.height,
            viewport_width,
            viewport_height,
            frame.window_id(),
            hit_testable as u8,
            frames.len() + 1,
            MAX_FRAMES,
        ),
    );
    frames.push(OpenFrame { frame, view, image });
}

fn aligned_position(
    output_width: u32,
    output_height: u32,
    width: u32,
    height: u32,
    alignment: Alignment,
) -> (i32, i32) {
    match alignment {
        Alignment::Center => (
            output_width.saturating_sub(width) as i32 / 2,
            output_height.saturating_sub(height) as i32 / 2,
        ),
        Alignment::TopLeft => (0, 0),
        Alignment::TopRight => (output_width.saturating_sub(width) as i32, 0),
        Alignment::BottomLeft => (0, output_height.saturating_sub(height) as i32),
        Alignment::BottomRight => (
            output_width.saturating_sub(width) as i32,
            output_height.saturating_sub(height) as i32,
        ),
    }
}

fn present(frame: &mut Frame, view: View, image: &Image) -> Result<(), Ui4Error> {
    let mut viewport = vec![0u8; view.viewport_width as usize * view.viewport_height as usize * 4];
    for alpha in viewport.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }

    if view.letterbox {
        paint_letterboxed(viewport.as_mut_slice(), view, image);
    } else {
        let source_x = (-view.offset_x).max(0.0) as usize;
        let source_y = (-view.offset_y).max(0.0) as usize;
        let destination_x = view.offset_x.max(0.0) as usize;
        let destination_y = view.offset_y.max(0.0) as usize;
        let copy_width = (image.width as usize)
            .saturating_sub(source_x)
            .min(view.viewport_width as usize - destination_x);
        let copy_height = (image.height as usize)
            .saturating_sub(source_y)
            .min(view.viewport_height as usize - destination_y);
        for row in 0..copy_height {
            let source_start = ((source_y + row) * image.width as usize + source_x) * 4;
            let destination_start =
                ((destination_y + row) * view.viewport_width as usize + destination_x) * 4;
            let byte_len = copy_width * 4;
            viewport[destination_start..destination_start + byte_len]
                .copy_from_slice(&image.rgba[source_start..source_start + byte_len]);
        }
    }

    frame.begin(rgba(0, 0, 0, 255))?;
    frame.write_opaque_rgba8(viewport.as_slice())?;
    frame.publish(Damage::full(frame.width(), frame.height()))
}

fn paint_letterboxed(viewport: &mut [u8], view: View, image: &Image) {
    let viewport_width = view.viewport_width as usize;
    let viewport_height = view.viewport_height as usize;
    let image_width = image.width as usize;
    let image_height = image.height as usize;
    let (draw_width, draw_height) =
        contained_extent(image_width, image_height, viewport_width, viewport_height);
    let destination_x = (viewport_width - draw_width) / 2;
    let destination_y = (viewport_height - draw_height) / 2;

    for draw_y in 0..draw_height {
        let source_y = draw_y * image_height / draw_height;
        for draw_x in 0..draw_width {
            let source_x = draw_x * image_width / draw_width;
            let source = (source_y * image_width + source_x) * 4;
            let destination =
                ((destination_y + draw_y) * viewport_width + destination_x + draw_x) * 4;
            viewport[destination..destination + 4].copy_from_slice(&image.rgba[source..source + 4]);
        }
    }
}

fn contained_extent(
    source_width: usize,
    source_height: usize,
    viewport_width: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if viewport_width.saturating_mul(source_height) <= viewport_height.saturating_mul(source_width)
    {
        (
            viewport_width,
            (source_height.saturating_mul(viewport_width) / source_width).max(1),
        )
    } else {
        (
            (source_width.saturating_mul(viewport_height) / source_height).max(1),
            viewport_height,
        )
    }
}

fn service_frames(frames: &mut Vec<OpenFrame>) {
    let mut index = 0;
    while index < frames.len() {
        let mut close = false;
        let mut repaint = false;
        {
            let open = &mut frames[index];
            loop {
                match open.frame.take_keyboard_event() {
                    Ok(Some(event))
                        if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                            && event.key_code == input::KEYBOARD_KEY_ESCAPE
                            && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0 =>
                    {
                        close = true;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(level::WARN, format_args!("img: keyboard error={error:?}"));
                        break;
                    }
                }
            }
            loop {
                match open.frame.take_pan_event() {
                    Ok(Some(event)) => {
                        open.view.pan(event.dx, event.dy);
                        repaint = true;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(level::WARN, format_args!("img: pan event error={error:?}"));
                        break;
                    }
                }
            }
            let mut resize = None;
            loop {
                match open.frame.take_resize_event() {
                    Ok(Some(event)) => resize = Some(event),
                    Ok(None) => break,
                    Err(error) => {
                        logl::log(
                            level::WARN,
                            format_args!("img: resize event error={error:?}"),
                        );
                        break;
                    }
                }
            }
            if let Some(event) = resize {
                match open.frame.resize(event.width, event.height) {
                    Ok(()) => {
                        open.view.resize(event.width, event.height);
                        repaint = true;
                        logl::log(
                            level::INFO,
                            format_args!(
                                "img: resize window={} old={}x{} new={}x{} letterbox={}",
                                open.frame.window_id(),
                                event.old_width,
                                event.old_height,
                                event.width,
                                event.height,
                                open.view.letterbox as u8,
                            ),
                        );
                    }
                    Err(error) => logl::log(
                        level::WARN,
                        format_args!(
                            "img: resize window={} old={}x{} requested={}x{} error={error:?}",
                            open.frame.window_id(),
                            event.old_width,
                            event.old_height,
                            event.width,
                            event.height,
                        ),
                    ),
                }
            }
            if repaint && let Err(error) = present(&mut open.frame, open.view, &open.image) {
                logl::log(level::WARN, format_args!("img: repaint error={error:?}"));
            }
        }
        if close {
            let closed = frames.swap_remove(index);
            logl::log(
                level::INFO,
                format_args!(
                    "img: close window={} source=escape",
                    closed.frame.window_id()
                ),
            );
            drop(closed);
        } else {
            index += 1;
        }
    }
}

fn load_image(source: &str) -> Result<Image, String> {
    if source.starts_with("kernel:") {
        let (info, bytes) =
            image_source::read(source).map_err(|code| format!("kernel source code={code}"))?;
        return match info.format {
            image_source::FORMAT_JPEG => decode_jpeg(bytes.as_slice()),
            image_source::FORMAT_RGBA8 => image_from_rgba(info.width, info.height, bytes),
            image_source::FORMAT_PNG => decode_png(bytes.as_slice()),
            _ => Err(String::from("unsupported kernel image format")),
        };
    }
    let bytes = async_fs::block_on(async_fs::read_file(source.as_bytes()))
        .map_err(|code| format!("trueosfs read code={code}"))?;
    decode_jpeg(bytes.as_slice())
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    let decoded = async_fs::block_on(vmedia::decode(vmedia::ImageFormat::Jpeg, bytes))
        .map_err(|code| format!("kernel JPEG decode code={code}"))?;
    let expected = checked_rgba_len(decoded.info.width, decoded.info.height)
        .ok_or_else(|| String::from("JPEG dimensions rejected"))?;
    if decoded.rgba.len() != expected {
        return Err(String::from("kernel JPEG decoded size mismatch"));
    }
    Ok(Image {
        width: decoded.info.width,
        height: decoded.info.height,
        rgba: decoded.rgba,
    })
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| String::from("invalid PNG"))?;
    let width = reader.info().width;
    let height = reader.info().height;
    let expected =
        checked_rgba_len(width, height).ok_or_else(|| String::from("PNG dimensions rejected"))?;
    let output_len = reader
        .output_buffer_size()
        .ok_or_else(|| String::from("PNG output too large"))?;
    if output_len > expected {
        return Err(String::from("PNG output layout rejected"));
    }
    let mut decoded = vec![0u8; output_len];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| String::from("PNG decode failed"))?;
    decoded.truncate(info.buffer_size());
    let pixels = expected / 4;
    let rgba = match info.color_type {
        png::ColorType::Rgba if decoded.len() == expected => decoded,
        png::ColorType::Rgb if decoded.len() == pixels * 3 => {
            let mut rgba = Vec::with_capacity(expected);
            for rgb in decoded.chunks_exact(3) {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], u8::MAX]);
            }
            rgba
        }
        png::ColorType::Grayscale if decoded.len() == pixels => {
            let mut rgba = Vec::with_capacity(expected);
            for gray in decoded {
                rgba.extend_from_slice(&[gray, gray, gray, u8::MAX]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha if decoded.len() == pixels * 2 => {
            let mut rgba = Vec::with_capacity(expected);
            for gray_alpha in decoded.chunks_exact(2) {
                rgba.extend_from_slice(&[
                    gray_alpha[0],
                    gray_alpha[0],
                    gray_alpha[0],
                    gray_alpha[1],
                ]);
            }
            rgba
        }
        _ => return Err(String::from("unsupported PNG output layout")),
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn image_from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Result<Image, String> {
    let expected =
        checked_rgba_len(width, height).ok_or_else(|| String::from("RGBA dimensions rejected"))?;
    if rgba.len() != expected {
        return Err(String::from("RGBA byte length mismatch"));
    }
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 || pixels > MAX_SOURCE_PIXELS {
        return None;
    }
    pixels.checked_mul(4)
}
