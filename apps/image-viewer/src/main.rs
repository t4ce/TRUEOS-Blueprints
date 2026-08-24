// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use core3::io::Cursor;
use trueos::ui4_scene::{
    CloseRequest, Damage, Error as Ui4Error, Frame, PanPhase, SpriteCorner, SpriteQuad,
    output_dimensions, rgba,
};
use trueos::{async_fs, env, logl, netfs, vshell, vsys};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

const BUILTIN_LOGO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/builtin-logo.jpg"));
const DEFAULT_SOURCE: &str = "builtin:logo";
const SPRITE_ID: u32 = 1;
const INPUT_POLL_MS: u64 = 8;
const NETWORK_TIMEOUT_MS: u64 = 30_000;
const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_PIXELS: usize = 64 * 1024 * 1024;
const MIN_ZOOM_PERCENT: i32 = 10;
const MAX_ZOOM_PERCENT: i32 = 1_600;
const ZOOM_STEP_PERCENT: i32 = 25;
const SHELL_INPUT_CAP: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageKind {
    Png,
    Jpeg,
    Bmp,
    Svg,
}

struct DecodedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    kind: ImageKind,
}

#[derive(Clone, Copy)]
struct View {
    viewport_width: u32,
    viewport_height: u32,
    image_width: u32,
    image_height: u32,
    zoom_percent: i32,
    offset_x: f32,
    offset_y: f32,
}

impl View {
    fn new(viewport_width: u32, viewport_height: u32, image_width: u32, image_height: u32) -> Self {
        let mut view = Self {
            viewport_width,
            viewport_height,
            image_width,
            image_height,
            zoom_percent: 100,
            offset_x: 0.0,
            offset_y: 0.0,
        };
        view.clamp_offsets();
        view
    }

    fn scaled_width(self) -> f32 {
        self.image_width as f32 * self.zoom_percent as f32 / 100.0
    }

    fn scaled_height(self) -> f32 {
        self.image_height as f32 * self.zoom_percent as f32 / 100.0
    }

    fn pan(&mut self, dx: i32, dy: i32) {
        self.offset_x += dx as f32;
        self.offset_y += dy as f32;
        self.clamp_offsets();
    }

    fn zoom_at(&mut self, wheel: i16, local_x: i32, local_y: i32) -> bool {
        if wheel == 0 {
            return false;
        }
        let direction = if wheel > 0 { 1 } else { -1 };
        let steps = i32::from(wheel.unsigned_abs()).clamp(1, 8);
        let old_zoom = self.zoom_percent;
        let new_zoom = (old_zoom + direction * ZOOM_STEP_PERCENT * steps)
            .clamp(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT);
        if new_zoom == old_zoom {
            return false;
        }

        let old_scale = old_zoom as f32 / 100.0;
        let new_scale = new_zoom as f32 / 100.0;
        let anchor_x = local_x.clamp(0, self.viewport_width as i32) as f32;
        let anchor_y = local_y.clamp(0, self.viewport_height as i32) as f32;
        let image_x = (anchor_x - self.offset_x) / old_scale;
        let image_y = (anchor_y - self.offset_y) / old_scale;
        self.zoom_percent = new_zoom;
        self.offset_x = anchor_x - image_x * new_scale;
        self.offset_y = anchor_y - image_y * new_scale;
        self.clamp_offsets();
        true
    }

    fn clamp_offsets(&mut self) {
        self.offset_x = clamp_axis(
            self.offset_x,
            self.viewport_width as f32,
            self.scaled_width(),
        );
        self.offset_y = clamp_axis(
            self.offset_y,
            self.viewport_height as f32,
            self.scaled_height(),
        );
    }

    fn quad(self) -> SpriteQuad {
        let left = self.offset_x;
        let top = self.offset_y;
        let right = left + self.scaled_width();
        let bottom = top + self.scaled_height();
        SpriteQuad {
            sprite_id: SPRITE_ID,
            c0: SpriteCorner {
                x: left,
                y: top,
                u: 0.0,
                v: 0.0,
            },
            c1: SpriteCorner {
                x: right,
                y: top,
                u: 1.0,
                v: 0.0,
            },
            c2: SpriteCorner {
                x: right,
                y: bottom,
                u: 1.0,
                v: 1.0,
            },
            c3: SpriteCorner {
                x: left,
                y: bottom,
                u: 0.0,
                v: 1.0,
            },
            color_rgba: rgba(255, 255, 255, 255),
            source_over: true,
        }
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
    let source = source_from_args();
    let image = match load_and_decode(source.as_str()) {
        Ok(image) => image,
        Err(error) => {
            logl::log(logl::level::ERROR, format_args!("image-viewer: {error}"));
            return;
        }
    };
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let viewport_width = image.width.min(output_width).max(1);
    let viewport_height = image.height.min(output_height).max(1);
    let origin_x = output_width.saturating_sub(viewport_width) as i32 / 2;
    let origin_y = output_height.saturating_sub(viewport_height) as i32 / 2;
    let Ok(mut frame) = Frame::open_immutable(origin_x, origin_y, viewport_width, viewport_height)
    else {
        logl::log(
            logl::level::ERROR,
            "image-viewer: UI4 immutable frame open failed",
        );
        return;
    };
    if let Err(error) =
        frame.upload_sprite_rgba8(SPRITE_ID, image.width, image.height, image.rgba.as_slice())
    {
        logl::log(
            logl::level::ERROR,
            format_args!("image-viewer: decoded RGBA upload failed error={error:?}"),
        );
        return;
    }

    let mut view = View::new(viewport_width, viewport_height, image.width, image.height);
    if let Err(error) = present(&mut frame, view) {
        logl::log(
            logl::level::ERROR,
            format_args!("image-viewer: initial present failed error={error:?}"),
        );
        return;
    }
    log_loaded(source.as_str(), &image, frame.window_id());

    let mut shell = ShellInput::new();
    loop {
        let mut changed = false;
        loop {
            match frame.take_pan_event() {
                Ok(Some(event)) => {
                    if matches!(event.phase, PanPhase::Begin | PanPhase::Update) {
                        view.pan(event.dx, event.dy);
                        changed = true;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    logl::log(
                        logl::level::WARN,
                        format_args!("image-viewer: pan input error={error:?}"),
                    );
                    break;
                }
            }
        }
        loop {
            match frame.take_pointer_event() {
                Ok(Some(event)) => {
                    changed |= view.zoom_at(event.wheel, event.local_x, event.local_y);
                }
                Ok(None) => break,
                Err(error) => {
                    logl::log(
                        logl::level::WARN,
                        format_args!("image-viewer: pointer input error={error:?}"),
                    );
                    break;
                }
            }
        }

        if changed {
            if let Err(error) = present(&mut frame, view) {
                logl::log(
                    logl::level::ERROR,
                    format_args!("image-viewer: repaint failed error={error:?}"),
                );
                return;
            }
        }

        if let Some(command) = shell.poll() {
            if command == "quit" {
                let _ = frame.close(CloseRequest::default());
                return;
            }
            if let Some(next_source) = command
                .strip_prefix("load ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                match load_and_decode(next_source) {
                    Ok(next) => {
                        if let Err(error) =
                            replace_image(&mut frame, &mut view, &next, output_width, output_height)
                        {
                            logl::log(
                                logl::level::ERROR,
                                format_args!("image-viewer: replacement failed error={error:?}"),
                            );
                        } else {
                            log_loaded(next_source, &next, frame.window_id());
                        }
                    }
                    Err(error) => {
                        logl::log(logl::level::WARN, format_args!("image-viewer: {error}"))
                    }
                }
            }
        }

        vsys::poll_once();
        vsys::sleep_ms(INPUT_POLL_MS);
    }
}

fn replace_image(
    frame: &mut Frame,
    view: &mut View,
    image: &DecodedImage,
    output_width: u32,
    output_height: u32,
) -> Result<(), Ui4Error> {
    let width = image.width.min(output_width).max(1);
    let height = image.height.min(output_height).max(1);
    if frame.width() != width || frame.height() != height {
        frame.resize(width, height)?;
        frame.set_position(
            output_width.saturating_sub(width) as i32 / 2,
            output_height.saturating_sub(height) as i32 / 2,
        )?;
    }
    frame.upload_sprite_rgba8(SPRITE_ID, image.width, image.height, image.rgba.as_slice())?;
    *view = View::new(width, height, image.width, image.height);
    present(frame, *view)
}

fn present(frame: &mut Frame, view: View) -> Result<(), Ui4Error> {
    loop {
        match frame.begin_sprite_frame(rgba(0, 0, 0, 0)) {
            Ok(()) => break,
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
    let quad = view.quad();
    loop {
        match frame.draw_sprite_quads(core::slice::from_ref(&quad)) {
            Ok(()) => break,
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
    loop {
        match frame.publish(Damage::full(frame.width(), frame.height())) {
            Ok(()) => return Ok(()),
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn source_from_args() -> String {
    let mut args = env::args();
    let _archive = args.next();
    args.next().unwrap_or_else(|| DEFAULT_SOURCE.to_string())
}

fn load_and_decode(source: &str) -> Result<DecodedImage, String> {
    let bytes = load_source(source)?;
    decode_image(source, bytes.as_slice())
}

fn load_source(source: &str) -> Result<Vec<u8>, String> {
    if source == "builtin:logo" || source == "logo" {
        return Ok(BUILTIN_LOGO.to_vec());
    }
    if source.starts_with("https://") || source.starts_with("http://") {
        let operation = netfs::fetch_bytes(source.as_bytes())
            .map_err(|code| format!("network start failed source={source} code={code}"))?;
        let wait = netfs::fetch_bytes_wait(operation, NETWORK_TIMEOUT_MS);
        if wait != 0 {
            let _ = netfs::fetch_bytes_discard(operation);
            return Err(format!("network fetch failed source={source} code={wait}"));
        }
        let result = netfs::fetch_bytes_read(operation)
            .map_err(|code| format!("network read failed source={source} code={code}"));
        let _ = netfs::fetch_bytes_discard(operation);
        let bytes = result?;
        validate_encoded_size(source, bytes.len())?;
        return Ok(bytes);
    }
    let bytes = async_fs::block_on(async_fs::read_file(source.as_bytes()))
        .map_err(|code| format!("filesystem read failed source={source} code={code}"))?;
    validate_encoded_size(source, bytes.len())?;
    Ok(bytes)
}

fn validate_encoded_size(source: &str, len: usize) -> Result<(), String> {
    if len == 0 || len > MAX_ENCODED_BYTES {
        Err(format!(
            "encoded image size rejected source={source} bytes={len}"
        ))
    } else {
        Ok(())
    }
}

fn decode_image(source: &str, bytes: &[u8]) -> Result<DecodedImage, String> {
    validate_encoded_size(source, bytes.len())?;
    match infer_kind(source, bytes).ok_or_else(|| format!("unsupported image source={source}"))? {
        ImageKind::Png => decode_png(bytes),
        ImageKind::Jpeg => decode_jpeg(bytes),
        ImageKind::Bmp => decode_bmp(bytes),
        ImageKind::Svg => Ok(svg_placeholder()),
    }
}

fn infer_kind(source: &str, bytes: &[u8]) -> Option<ImageKind> {
    let lower = source.to_ascii_lowercase();
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || lower.ends_with(".png") {
        return Some(ImageKind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
    {
        return Some(ImageKind::Jpeg);
    }
    if bytes.starts_with(b"BM") || lower.ends_with(".bmp") {
        return Some(ImageKind::Bmp);
    }
    let prefix = bytes
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take(256)
        .collect::<Vec<_>>();
    if lower.ends_with(".svg")
        || prefix.starts_with(b"<svg")
        || prefix
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"<svg"))
    {
        return Some(ImageKind::Svg);
    }
    None
}

fn checked_rgba_len(width: u32, height: u32) -> Result<usize, String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "image dimensions overflow".to_string())?;
    if width == 0 || height == 0 || pixels > MAX_PIXELS {
        return Err(format!(
            "image dimensions rejected width={width} height={height}"
        ));
    }
    pixels
        .checked_mul(4)
        .ok_or_else(|| "RGBA size overflow".to_string())
}

fn decode_png(bytes: &[u8]) -> Result<DecodedImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| "invalid PNG".to_string())?;
    let width = reader.info().width;
    let height = reader.info().height;
    let expected = checked_rgba_len(width, height)?;
    let output_len = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG output too large".to_string())?;
    if output_len > expected {
        return Err("PNG output layout rejected".to_string());
    }
    let mut decoded = vec![0u8; output_len];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| "PNG decode failed".to_string())?;
    decoded.truncate(info.buffer_size());
    let rgba = expand_png_to_rgba(info.color_type, width, height, decoded)?;
    Ok(DecodedImage {
        width,
        height,
        rgba,
        kind: ImageKind::Png,
    })
}

fn expand_png_to_rgba(
    color_type: png::ColorType,
    width: u32,
    height: u32,
    decoded: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let expected = checked_rgba_len(width, height)?;
    let pixels = expected / 4;
    match color_type {
        png::ColorType::Rgba if decoded.len() == expected => Ok(decoded),
        png::ColorType::Rgb if decoded.len() == pixels * 3 => {
            let mut rgba = Vec::with_capacity(expected);
            for rgb in decoded.chunks_exact(3) {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            Ok(rgba)
        }
        png::ColorType::Grayscale if decoded.len() == pixels => {
            let mut rgba = Vec::with_capacity(expected);
            for gray in decoded {
                rgba.extend_from_slice(&[gray, gray, gray, 255]);
            }
            Ok(rgba)
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
            Ok(rgba)
        }
        _ => Err("unsupported PNG output layout".to_string()),
    }
}

fn decode_jpeg(bytes: &[u8]) -> Result<DecodedImage, String> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_use_unsafe(true);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|_| "invalid JPEG".to_string())?;
    let info = decoder
        .info()
        .ok_or_else(|| "JPEG has no dimensions".to_string())?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let expected = checked_rgba_len(width, height)?;
    let rgba = decoder
        .decode()
        .map_err(|_| "JPEG decode failed".to_string())?;
    if rgba.len() != expected {
        return Err("JPEG decoded size mismatch".to_string());
    }
    Ok(DecodedImage {
        width,
        height,
        rgba,
        kind: ImageKind::Jpeg,
    })
}

fn decode_bmp(bytes: &[u8]) -> Result<DecodedImage, String> {
    if bytes.len() < 54 || !bytes.starts_with(b"BM") {
        return Err("invalid BMP".to_string());
    }
    let data_offset = le_u32(bytes, 10)? as usize;
    let dib_size = le_u32(bytes, 14)?;
    if dib_size < 40 {
        return Err("unsupported BMP DIB header".to_string());
    }
    let width_i = le_i32(bytes, 18)?;
    let height_i = le_i32(bytes, 22)?;
    let planes = le_u16(bytes, 26)?;
    let bits = le_u16(bytes, 28)?;
    let compression = le_u32(bytes, 30)?;
    if width_i <= 0 || height_i == 0 || planes != 1 || compression != 0 || !matches!(bits, 24 | 32)
    {
        return Err("unsupported BMP layout (need uncompressed 24/32-bit)".to_string());
    }
    let width = width_i as u32;
    let height = height_i.unsigned_abs();
    let output_len = checked_rgba_len(width, height)?;
    let bytes_per_pixel = usize::from(bits / 8);
    let row_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| "BMP row overflow".to_string())?;
    let stride = row_bytes
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| "BMP stride overflow".to_string())?;
    let source_len = stride
        .checked_mul(height as usize)
        .ok_or_else(|| "BMP size overflow".to_string())?;
    if data_offset
        .checked_add(source_len)
        .is_none_or(|end| end > bytes.len())
    {
        return Err("truncated BMP pixels".to_string());
    }
    let top_down = height_i < 0;
    let mut rgba = Vec::with_capacity(output_len);
    let mut all_alpha_zero = bits == 32;
    for output_y in 0..height as usize {
        let source_y = if top_down {
            output_y
        } else {
            height as usize - 1 - output_y
        };
        let row = data_offset + source_y * stride;
        for x in 0..width as usize {
            let pixel = row + x * bytes_per_pixel;
            let blue = bytes[pixel];
            let green = bytes[pixel + 1];
            let red = bytes[pixel + 2];
            let alpha = if bits == 32 { bytes[pixel + 3] } else { 255 };
            all_alpha_zero &= alpha == 0;
            rgba.extend_from_slice(&[red, green, blue, alpha]);
        }
    }
    if all_alpha_zero {
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }
    Ok(DecodedImage {
        width,
        height,
        rgba,
        kind: ImageKind::Bmp,
    })
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "truncated BMP header".to_string())?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated BMP header".to_string())?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn le_i32(bytes: &[u8], offset: usize) -> Result<i32, String> {
    Ok(le_u32(bytes, offset)? as i32)
}

fn svg_placeholder() -> DecodedImage {
    let width = 256;
    let height = 256;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 0, 255, 255]);
    }
    DecodedImage {
        width,
        height,
        rgba,
        kind: ImageKind::Svg,
    }
}

fn log_loaded(source: &str, image: &DecodedImage, window: u32) {
    logl::log(
        logl::level::INFO,
        format_args!(
            "image-viewer: loaded source={} kind={:?} image={}x{} window={} initial_zoom=100% pan=middle-button zoom=wheel close=Escape reload='load <source>'",
            source, image.kind, image.width, image.height, window,
        ),
    );
}

struct ShellInput {
    bytes: [u8; SHELL_INPUT_CAP],
    len: usize,
}

impl ShellInput {
    const fn new() -> Self {
        Self {
            bytes: [0; SHELL_INPUT_CAP],
            len: 0,
        }
    }

    fn poll(&mut self) -> Option<String> {
        while let Some(byte) = vshell::attached_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    let len = self.len;
                    self.len = 0;
                    return String::from_utf8(self.bytes[..len].to_vec()).ok();
                }
                8 | 127 => self.len = self.len.saturating_sub(1),
                0x20..=0x7e if self.len < self.bytes.len() => {
                    self.bytes[self.len] = byte;
                    self.len += 1;
                }
                _ => {}
            }
        }
        None
    }
}
