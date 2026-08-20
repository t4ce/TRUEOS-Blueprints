#![no_std]

use trueos::input::{
    KEYBOARD_KEY_ARROW_LEFT, KEYBOARD_KEY_ARROW_RIGHT, KEYBOARD_KEY_ESCAPE, KEYBOARD_KEY_F1,
    KEYBOARD_KEY_F2, KEYBOARD_KEY_F3, KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY,
};
use trueos::ui4_scene::{
    Error, Frame, SHADERTOY_CUBE_FIELD, SHADERTOY_MANDELBROT, SHADERTOY_NGUYEN, ShadertoyParamsV1,
};
use trueos::{clock, logl, vsys};

const FRAME_X: i32 = 480;
const FRAME_Y: i32 = 96;
const INITIAL_WIDTH: u32 = 640;
const INITIAL_HEIGHT: u32 = 360;
const TARGET_HZ: u32 = 30;
const SAMPLE_RATE: f32 = 44_100.0;

#[derive(Copy, Clone)]
struct Shader {
    id: u32,
    name: &'static str,
    artifact_sha256: &'static str,
}

// The app ships this admitted catalog. Executable bytes remain kernel-owned,
// so the Blueprint cannot inject a pointer, SPIR-V module, or unreviewed Zebin
// through the provisional visual ABI.
const SHADERS: [Shader; 3] = [
    Shader {
        id: SHADERTOY_MANDELBROT,
        name: "Mandelbrot zoom",
        artifact_sha256: "79e566ad2db01a1a2467e0289bd97e9c77c67be7bd4a59d957dadd84e0ec32d1",
    },
    Shader {
        id: SHADERTOY_CUBE_FIELD,
        name: "Raymarched cube field",
        artifact_sha256: "0d48ef4d170eafe0cec5ae3952abdc6e57e865b195dbc3fc137ca7eb1b25d736",
    },
    Shader {
        id: SHADERTOY_NGUYEN,
        name: "Nguyen compact visual",
        artifact_sha256: "1dbc80b468dd896073dd17c3963a5c7cccf814365e21f040e05a3522fea4cd9c",
    },
];

fn main() {
    logl::log(
        logl::level::INFO,
        "shadertoy: opening UI4 visual mode at target 30 Hz (kernel ceiling 60 Hz)",
    );
    let Ok(mut frame) =
        Frame::open_visual(FRAME_X, FRAME_Y, INITIAL_WIDTH, INITIAL_HEIGHT, TARGET_HZ)
    else {
        logl::log(logl::level::ERROR, "shadertoy: visual frame open failed");
        return;
    };

    let mut shader_index = 0usize;
    let mut shader_started_ns = clock::monotonic_nanos();
    let mut previous_ns = shader_started_ns;
    let mut frame_number = 0u32;
    let mut width = INITIAL_WIDTH;
    let mut height = INITIAL_HEIGHT;
    let mut mouse = [0.0f32; 4];
    log_shader(SHADERS[shader_index]);

    'running: loop {
        vsys::poll_once();

        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => {
                    if event.width != width || event.height != height {
                        if let Err(error) = frame.resize(event.width, event.height) {
                            fail("resize", error);
                            return;
                        }
                        width = event.width;
                        height = event.height;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    fail("resize event", error);
                    return;
                }
            }
        }

        loop {
            match frame.take_pointer_event() {
                Ok(Some(event)) => {
                    mouse[0] = (event.local_x as f32).clamp(0.0, width as f32);
                    mouse[1] = (height as f32 - event.local_y as f32).clamp(0.0, height as f32);
                    if event.buttons_pressed != 0 {
                        mouse[2] = mouse[0];
                        mouse[3] = mouse[1];
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    fail("pointer event", error);
                    return;
                }
            }
        }

        loop {
            let event = match frame.take_keyboard_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(error) => {
                    fail("keyboard event", error);
                    return;
                }
            };
            if event.kind != KEYBOARD_OUTPUT_KIND_KEY
                || event.flags & KEYBOARD_OUTPUT_FLAG_PRESS == 0
            {
                continue;
            }
            let next = match event.key_code {
                KEYBOARD_KEY_ESCAPE => break 'running,
                KEYBOARD_KEY_ARROW_LEFT => Some((shader_index + SHADERS.len() - 1) % SHADERS.len()),
                KEYBOARD_KEY_ARROW_RIGHT => Some((shader_index + 1) % SHADERS.len()),
                KEYBOARD_KEY_F1 => Some(0),
                KEYBOARD_KEY_F2 => Some(1),
                KEYBOARD_KEY_F3 => Some(2),
                _ => None,
            };
            if let Some(next) = next
                && next != shader_index
            {
                shader_index = next;
                shader_started_ns = clock::monotonic_nanos();
                previous_ns = shader_started_ns;
                frame_number = 0;
                log_shader(SHADERS[shader_index]);
            }
        }

        if let Err(error) = frame.begin_visual_gpu_frame() {
            fail("frame begin", error);
            return;
        }

        let now_ns = clock::monotonic_nanos();
        let date = clock::utc_date_time();
        let params = ShadertoyParamsV1 {
            shader_id: SHADERS[shader_index].id,
            frame: frame_number,
            time_seconds: now_ns.saturating_sub(shader_started_ns) as f32 / 1_000_000_000.0,
            delta_seconds: now_ns.saturating_sub(previous_ns) as f32 / 1_000_000_000.0,
            frame_rate: TARGET_HZ as f32,
            sample_rate: SAMPLE_RATE,
            mouse_x: mouse[0],
            mouse_y: mouse[1],
            click_x: mouse[2],
            click_y: mouse[3],
            date_year: date.map_or(0.0, |value| value.year as f32),
            date_month: date.map_or(0.0, |value| value.month as f32),
            date_day: date.map_or(0.0, |value| value.day as f32),
            date_seconds: date.map_or(0.0, |value| {
                (u32::from(value.hour) * 3_600
                    + u32::from(value.minute) * 60
                    + u32::from(value.second)) as f32
            }),
        };
        if let Err(error) = frame.render_shadertoy(&params) {
            fail("render/compute publish", error);
            return;
        }
        previous_ns = now_ns;
        frame_number = frame_number.wrapping_add(1);
    }
}

fn log_shader(shader: Shader) {
    logl::log(
        logl::level::INFO,
        format_args!(
            "shadertoy: selected '{}' id={} artifact_sha256={} controls=Left/Right,F1-F3,Esc",
            shader.name, shader.id, shader.artifact_sha256
        ),
    );
}

fn fail(stage: &str, error: Error) {
    logl::log(
        logl::level::ERROR,
        format_args!("shadertoy: {stage} failed: {error:?}"),
    );
}
