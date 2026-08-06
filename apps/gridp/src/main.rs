// trueos-blueprint: features=["ui4-scene"]
#![no_std]

use trueos::ui4_scene::{
    CloseRequest, Damage, Error as Ui4Error, Font, Frame, SceneTextRow, output_dimensions, rgba,
};
use trueos::{env, logl, vshell, vsys};

const FRAME_WIDTH: u32 = 480;
const FRAME_HEIGHT: u32 = 320;
const FRAME_RGBA: u32 = rgba(92, 96, 102, 255);
const LABEL_FONT_SIZE: f32 = 48.0;
const LABEL_COLOR: u32 = rgba(255, 255, 255, 255);
const POLL_MS: u64 = 16;

fn main() {
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let width = FRAME_WIDTH.min(output_width).max(1);
    let height = FRAME_HEIGHT.min(output_height).max(1);
    let x = output_width.saturating_sub(width) as i32 / 2;
    let y = output_height.saturating_sub(height) as i32 / 2;

    let Ok(mut frame) = Frame::open_immutable(x, y, width, height) else {
        logl::log(logl::level::ERROR, "gridp: UI4 frame open failed");
        return;
    };
    let instance_name = env::var("TRUEOS_APP_INSTANCE_NAME").ok();
    let instance_name = match instance_name.as_deref() {
        Some(name) if !name.is_empty() => name,
        _ => "container",
    };
    if let Err(error) = present_gray_frame(&mut frame, instance_name) {
        logl::log(
            logl::level::ERROR,
            format_args!("gridp: initial frame publish failed error={error:?}"),
        );
        return;
    }

    logl::log(
        logl::level::INFO,
        format_args!(
            "gridp: frame submitted window={} extent={}x{} position={},{} interaction=ui4-movable",
            frame.window_id(),
            width,
            height,
            x,
            y,
        ),
    );

    let mut shell_input = [0u8; 32];
    loop {
        if frame.take_first_presentation().unwrap_or(false) {
            logl::log(
                logl::level::INFO,
                format_args!("gridp: frame visible window={}", frame.window_id()),
            );
        }
        let read = vshell::read(&mut shell_input);
        if trim_ascii(&shell_input[..read]) == b"quit" {
            let _ = frame.close(CloseRequest::default());
            return;
        }
        vsys::poll_once();
        vsys::sleep_ms(POLL_MS);
    }
}

fn present_gray_frame(frame: &mut Frame, label: &str) -> Result<(), Ui4Error> {
    retry_busy(|| frame.begin(FRAME_RGBA))?;
    let label = label.trim().trim_end_matches('\0');
    let label = if label.is_empty() { "gridp" } else { label };
    let rows = [SceneTextRow {
        text: label,
        x: center_label_x(frame.width(), LABEL_FONT_SIZE, label),
        y: center_label_y(frame.height(), LABEL_FONT_SIZE),
        font_pixels: LABEL_FONT_SIZE,
    }];
    retry_busy(|| {
        frame.stamp_text_scene(
            Font::NotoSansSc,
            (frame.width(), frame.height()),
            LABEL_COLOR,
            &rows,
        )
    })?;
    retry_busy(|| frame.publish(Damage::full(frame.width(), frame.height())))
}

fn center_label_x(frame_width: u32, font_pixels: f32, label: &str) -> f32 {
    let chars = label.chars().count() as f32;
    let glyph_advance = font_pixels * 0.55;
    let width_estimate = if chars > 0.0 {
        chars * glyph_advance
    } else {
        font_pixels
    };
    ((frame_width as f32 - width_estimate) * 0.5).max(0.0)
}

fn center_label_y(frame_height: u32, font_pixels: f32) -> f32 {
    ((frame_height as f32 - font_pixels) * 0.5).max(0.0)
}

fn retry_busy(mut operation: impl FnMut() -> Result<(), Ui4Error>) -> Result<(), Ui4Error> {
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
