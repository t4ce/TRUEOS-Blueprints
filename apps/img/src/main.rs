// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, output_dimensions, rgba};
use trueos::vsys;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

const BUILTIN_LOGO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/builtin-logo.jpg"));
const FRAME_MAX_WIDTH: u32 = 1_280;
const FRAME_MAX_HEIGHT: u32 = 720;
const MAX_SOURCE_PIXELS: usize = 64 * 1024 * 1024;

struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn main() {
    let image = match decode_builtin_logo() {
        Ok(image) => image,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("img: decode failed error={error}"),
            );
            return;
        }
    };
    let source_width = image.width;
    let source_height = image.height;
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let Some((frame_width, frame_height)) = fit_dimensions(
        source_width,
        source_height,
        FRAME_MAX_WIDTH.min(output_width),
        FRAME_MAX_HEIGHT.min(output_height),
    ) else {
        logl::log(level::ERROR, "img: invalid source or output dimensions");
        return;
    };
    let pixels = match scale_opaque_rgba8(image, frame_width, frame_height) {
        Ok(pixels) => pixels,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("img: scale failed error={error}"),
            );
            return;
        }
    };
    let origin_x = output_width.saturating_sub(frame_width) as i32 / 2;
    let origin_y = output_height.saturating_sub(frame_height) as i32 / 2;
    logl::log(
        level::INFO,
        format_args!(
            "img: decoded source=builtin:logo kind=Jpeg image={}x{} frame={}x{} bytes={} path=cpu-opaque-rgba8",
            source_width,
            source_height,
            frame_width,
            frame_height,
            pixels.len(),
        ),
    );
    let mut frame = match Frame::open(origin_x, origin_y, frame_width, frame_height) {
        Ok(frame) => frame,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("img: frame open failed error={error:?}"),
            );
            return;
        }
    };

    if let Err(error) = begin_frame(&mut frame) {
        logl::log(
            level::ERROR,
            format_args!("img: frame begin failed error={error:?}"),
        );
        return;
    }
    let pixel_bytes = pixels.len();
    if let Err(error) = frame.write_opaque_rgba8(pixels.as_slice()) {
        logl::log(
            level::ERROR,
            format_args!("img: RGBA write failed error={error:?}"),
        );
        return;
    }
    drop(pixels);
    if let Err(error) = frame.publish(Damage::full(frame_width, frame_height)) {
        logl::log(
            level::ERROR,
            format_args!("img: frame publish failed error={error:?}"),
        );
        return;
    }

    logl::log(
        level::INFO,
        format_args!(
            "img: published source=builtin:logo kind=Jpeg image={}x{} frame={}x{} bytes={} window={} path=cpu-opaque-rgba8; awaiting first presentation",
            source_width,
            source_height,
            frame_width,
            frame_height,
            pixel_bytes,
            frame.window_id(),
        ),
    );
    if let Err(error) = wait_for_first_presentation(&mut frame) {
        logl::log(
            level::ERROR,
            format_args!("img: first-presentation wait failed error={error:?}"),
        );
        return;
    }
    logl::log(
        level::INFO,
        format_args!(
            "img: presented source=builtin:logo frame={}x{} window={} path=cpu-opaque-rgba8",
            frame_width,
            frame_height,
            frame.window_id(),
        ),
    );

    loop {
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

fn decode_builtin_logo() -> Result<Image, &'static str> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_use_unsafe(true);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(BUILTIN_LOGO), options);
    decoder
        .decode_headers()
        .map_err(|_| "invalid built-in JPEG")?;
    let info = decoder.info().ok_or("built-in JPEG has no dimensions")?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let expected = checked_rgba_len(width, height).ok_or("built-in JPEG dimensions rejected")?;
    let rgba = decoder
        .decode()
        .map_err(|_| "built-in JPEG decode failed")?;
    if rgba.len() != expected {
        return Err("built-in JPEG decoded size mismatch");
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn fit_dimensions(
    source_width: u32,
    source_height: u32,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32)> {
    if source_width == 0 || source_height == 0 || max_width == 0 || max_height == 0 {
        return None;
    }
    let width_limited_height =
        (u64::from(source_height) * u64::from(max_width) / u64::from(source_width)) as u32;
    if width_limited_height <= max_height {
        Some((max_width, width_limited_height.max(1)))
    } else {
        let height_limited_width =
            (u64::from(source_width) * u64::from(max_height) / u64::from(source_height)) as u32;
        Some((height_limited_width.max(1), max_height))
    }
}

fn scale_opaque_rgba8(
    image: Image,
    target_width: u32,
    target_height: u32,
) -> Result<Vec<u8>, &'static str> {
    let output_len = checked_rgba_len(target_width, target_height).ok_or("frame too large")?;
    let mut output = vec![0u8; output_len];
    let source_width = image.width as usize;
    let source_height = image.height as usize;
    let target_width = target_width as usize;
    let target_height = target_height as usize;

    for target_y in 0..target_height {
        let source_y = target_y * source_height / target_height;
        for target_x in 0..target_width {
            let source_x = target_x * source_width / target_width;
            let source = (source_y * source_width + source_x) * 4;
            let target = (target_y * target_width + target_x) * 4;
            output[target] = image.rgba[source];
            output[target + 1] = image.rgba[source + 1];
            output[target + 2] = image.rgba[source + 2];
            output[target + 3] = u8::MAX;
        }
    }
    Ok(output)
}

fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 || pixels > MAX_SOURCE_PIXELS {
        return None;
    }
    pixels.checked_mul(4)
}

fn begin_frame(frame: &mut Frame) -> Result<(), Ui4Error> {
    loop {
        match frame.begin(rgba(0, 0, 0, 255)) {
            Ok(()) => return Ok(()),
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn wait_for_first_presentation(frame: &mut Frame) -> Result<(), Ui4Error> {
    loop {
        if frame.take_first_presentation()? {
            return Ok(());
        }
        vsys::poll_once();
        vsys::sleep_ms(1);
    }
}
