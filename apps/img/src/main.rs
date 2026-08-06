// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

use alloc::{format, string::{String, ToString}, vec, vec::Vec};
use core3::io::Cursor;
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, output_dimensions, rgba};
use trueos::{async_fs, image_source, input, vsys};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

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
    image_width: u32,
    image_height: u32,
    offset_x: f32,
    offset_y: f32,
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
            image_width,
            image_height,
            offset_x,
            offset_y,
        };
        view.clamp_offsets();
        view
    }

    fn pan(&mut self, dx: i32, dy: i32) {
        self.offset_x += dx as f32;
        self.offset_y += dy as f32;
        self.clamp_offsets();
    }

    fn clamp_offsets(&mut self) {
        self.offset_x = clamp_axis(self.offset_x, self.viewport_width as f32, self.image_width as f32);
        self.offset_y = clamp_axis(self.offset_y, self.viewport_height as f32, self.image_height as f32);
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
    logl::log(level::INFO, format_args!("img: vFile launch read begin"));
    let script = launch_script();
    logl::log(
        level::INFO,
        format_args!("img: vFile launch read ok bytes={} lines={}", script.len(), script.lines().count()),
    );
    for line in script.lines() {
        run_line(line, &mut frames);
    }

    loop {
        service_frames(&mut frames);
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

fn launch_script() -> String {
    match async_fs::block_on(async_fs::read_file(b"vFile:launch")) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(script) => script,
            Err(_) => {
                logl::log(level::ERROR, format_args!("img: vFile launch invalid UTF-8"));
                String::new()
            }
        },
        Err(code) => {
            logl::log(level::ERROR, format_args!("img: vFile launch read code={code}"));
            String::new()
        }
    }
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
                logl::log(level::WARN, format_args!("img: unknown show option={option}"));
                return;
            }
        }
    }
    if frames.len() >= MAX_FRAMES {
        logl::log(level::WARN, format_args!("img: frame cap={} source={path}", MAX_FRAMES));
        return;
    }
    let image = match load_image(path.trim()) {
        Ok(image) => image,
        Err(error) => {
            logl::log(level::ERROR, format_args!("img: show source={path} error={error}"));
            return;
        }
    };
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let viewport_width = image.width.min(output_width).max(1);
    let viewport_height = image.height.min(output_height).max(1);
    let (x, y) = aligned_position(output_width, output_height, viewport_width, viewport_height, alignment);
    let mut frame = match Frame::open_immutable(x, y, viewport_width, viewport_height) {
        Ok(frame) => frame,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("img: viewport rejected source={path} image={}x{} viewport={}x{} error={error:?}", image.width, image.height, viewport_width, viewport_height),
            );
            return;
        }
    };
    if let Err(error) = frame.set_hit_testable(hit_testable) {
        logl::log(level::ERROR, format_args!("img: hit-test source={path} error={error:?}"));
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
        logl::log(level::ERROR, format_args!("img: present source={path} error={error:?}"));
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
        let destination_start = ((destination_y + row) * view.viewport_width as usize + destination_x) * 4;
        let byte_len = copy_width * 4;
        viewport[destination_start..destination_start + byte_len]
            .copy_from_slice(&image.rgba[source_start..source_start + byte_len]);
    }

    frame.begin(rgba(0, 0, 0, 255))?;
    frame.write_opaque_rgba8(viewport.as_slice())?;
    frame.publish(Damage::full(frame.width(), frame.height()))
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
                    Ok(Some(event)) if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                        && event.key_code == input::KEYBOARD_KEY_ESCAPE
                        && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0 => {
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
            while open.frame.take_resize_event().ok().flatten().is_some() {}
            if repaint && let Err(error) = present(&mut open.frame, open.view, &open.image) {
                logl::log(level::WARN, format_args!("img: pan repaint error={error:?}"));
            }
        }
        if close {
            let closed = frames.swap_remove(index);
            logl::log(level::INFO, format_args!("img: close window={} source=escape", closed.frame.window_id()));
            drop(closed);
        } else {
            index += 1;
        }
    }
}

fn load_image(source: &str) -> Result<Image, String> {
    if source.starts_with("kernel:") {
        let (info, bytes) = image_source::read(source).map_err(|code| format!("kernel source code={code}"))?;
        return match info.format {
            image_source::FORMAT_JPEG => decode_jpeg(bytes.as_slice()).map_err(String::from),
            image_source::FORMAT_RGBA8 => image_from_rgba(info.width, info.height, bytes),
            image_source::FORMAT_PNG => decode_png(bytes.as_slice()),
            _ => Err(String::from("unsupported kernel image format")),
        };
    }
    let bytes = async_fs::block_on(async_fs::read_file(source.as_bytes()))
        .map_err(|code| format!("trueosfs read code={code}"))?;
    decode_jpeg(bytes.as_slice()).map_err(String::from)
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, &'static str> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_use_unsafe(true);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|_| "invalid JPEG")?;
    let info = decoder.info().ok_or("JPEG has no dimensions")?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let expected = checked_rgba_len(width, height).ok_or("JPEG dimensions rejected")?;
    let rgba = decoder
        .decode()
        .map_err(|_| "JPEG decode failed")?;
    if rgba.len() != expected {
        return Err("built-in JPEG decoded size mismatch");
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| String::from("invalid PNG"))?;
    let width = reader.info().width;
    let height = reader.info().height;
    let expected = checked_rgba_len(width, height).ok_or_else(|| String::from("PNG dimensions rejected"))?;
    let output_len = reader.output_buffer_size().ok_or_else(|| String::from("PNG output too large"))?;
    if output_len > expected {
        return Err(String::from("PNG output layout rejected"));
    }
    let mut decoded = vec![0u8; output_len];
    let info = reader.next_frame(&mut decoded).map_err(|_| String::from("PNG decode failed"))?;
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
                rgba.extend_from_slice(&[gray_alpha[0], gray_alpha[0], gray_alpha[0], gray_alpha[1]]);
            }
            rgba
        }
        _ => return Err(String::from("unsupported PNG output layout")),
    };
    Ok(Image { width, height, rgba })
}

fn image_from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Result<Image, String> {
    let expected = checked_rgba_len(width, height).ok_or_else(|| String::from("RGBA dimensions rejected"))?;
    if rgba.len() != expected {
        return Err(String::from("RGBA byte length mismatch"));
    }
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = u8::MAX;
    }
    Ok(Image { width, height, rgba })
}

fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 || pixels > MAX_SOURCE_PIXELS {
        return None;
    }
    pixels.checked_mul(4)
}
