#![no_std]

use trueos::input::{
    KEYBOARD_KEY_ARROW_LEFT, KEYBOARD_KEY_ARROW_RIGHT, KEYBOARD_KEY_F1, KEYBOARD_KEY_F2,
    KEYBOARD_KEY_SPACE, KEYBOARD_KEY_F3, KEYBOARD_KEY_F4, KEYBOARD_KEY_F5, KEYBOARD_KEY_F6, KEYBOARD_OUTPUT_FLAG_PRESS,
    KEYBOARD_OUTPUT_KIND_KEY,
};
use trueos::ui4_scene::{
    Error, Frame, SHADERTOY_FLAG_NATIVE_RESOLUTION, SHADERTOY_CUBE_FIELD, SHADERTOY_MANDELBROT, SHADERTOY_NGUYEN,
    SHADERTOY_COSMIC_STRANDS, SHADERTOY_PROTEAN_CLOUDS, SHADERTOY_PALETTE_GRID, ShadertoyParamsV1,
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
    package: &'static [u8],
}

// Pending sources are in assets/candidates/hex_array_pulse/input.glsl (scratch)
// and assets/candidates/aiekick_sphere/input.glsl (texture/cubemap channel ABI).
// They have no executable catalog ID until those requirements are supported.
// Packages contain the executable, SPIR-V, raw GLSL, generated C++ and bake
// provenance. Kernel-owned hashes/contracts remain the authority for admission.
const SHADERS: [Shader; 6] = [
    Shader {
        id: SHADERTOY_MANDELBROT,
        package: include_bytes!("../assets/mandelbrot.stpkg"),
        name: "Mandelbrot zoom",
        artifact_sha256: "79e566ad2db01a1a2467e0289bd97e9c77c67be7bd4a59d957dadd84e0ec32d1",
    },
    Shader {
        id: SHADERTOY_CUBE_FIELD,
        package: include_bytes!("../assets/cube_field.stpkg"),
        name: "Animated cube field",
        artifact_sha256: "04f940ae84746975d6c11033ce7899ccc8307badcaf3091f53a654ca10256f10",
    },
    Shader {
        id: SHADERTOY_NGUYEN,
        package: include_bytes!("../assets/nguyen.stpkg"),
        name: "Nguyen compact visual",
        artifact_sha256: "7140703571a20d5640876caddbe5948aa84f8828ff1d621b6eae1ef7d67af54d",
    },
    Shader {
        id: SHADERTOY_PALETTE_GRID,
        package: include_bytes!("../assets/palette_grid.stpkg"),
        name: "Palette grid glow",
        artifact_sha256: "2174c3002ff5e0c489de3ea4aff8da5b922b995e6075967a326eeb656e280124",
    },
    Shader {
        id: SHADERTOY_COSMIC_STRANDS,
        package: include_bytes!("../assets/cosmic_strands.stpkg"),
        name: "Cosmic Strands",
        artifact_sha256: "bf7e5b8a590526a36fa9684a4055d9dd255e36cba8b5ab75813ca3b59b4569d4",
    },
    Shader {
        id: SHADERTOY_PROTEAN_CLOUDS,
        package: include_bytes!("../assets/protean_clouds.stpkg"),
        name: "Protean Clouds",
        artifact_sha256: "aad75d1acb31ae065420ee907d5c2bcbe9bb73b71f29c27943a0ec1504956e56",
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

    for shader in SHADERS {
        if let Err(error) = frame.register_shadertoy(shader.id, shader.package) {
            log_shader(shader);
            fail("package registration", error);
            return;
        }
    }

    let mut shader_index = 0usize;
    let mut native_resolution = false;
    let mut shader_started_ns = clock::monotonic_nanos();
    let mut previous_ns = shader_started_ns;
    let mut frame_number = 0u32;
    let mut width = INITIAL_WIDTH;
    let mut height = INITIAL_HEIGHT;
    let mut mouse = [0.0f32; 4];
    let mut stats_started_ns = shader_started_ns;
    let mut stats_frames = 0u64;
    let mut stats_render_ns = 0u64;
    let mut stats_max_render_ns = 0u64;
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
                        stats_started_ns = clock::monotonic_nanos();
                        stats_frames = 0;
                        stats_render_ns = 0;
                        stats_max_render_ns = 0;
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
            if event.key_code == KEYBOARD_KEY_SPACE && SHADERS[shader_index].id == SHADERTOY_PROTEAN_CLOUDS {
                native_resolution = !native_resolution;
                logl::log(logl::level::INFO, format_args!("shadertoy: clouds sampling={}",
                    if native_resolution { "native" } else { "radial-auto" }));
                stats_started_ns = clock::monotonic_nanos();
                stats_frames = 0;
                stats_render_ns = 0;
                stats_max_render_ns = 0;
            }
            let next = match event.key_code {
                KEYBOARD_KEY_ARROW_LEFT => Some((shader_index + SHADERS.len() - 1) % SHADERS.len()),
                KEYBOARD_KEY_ARROW_RIGHT => Some((shader_index + 1) % SHADERS.len()),
                KEYBOARD_KEY_F1 => Some(0),
                KEYBOARD_KEY_F2 => Some(1),
                KEYBOARD_KEY_F3 => Some(2),
                KEYBOARD_KEY_F4 => Some(3),
                KEYBOARD_KEY_F5 => Some(4),
                KEYBOARD_KEY_F6 => Some(5),
                _ => None,
            };
            if let Some(next) = next
                && next != shader_index
            {
                shader_index = next;
                shader_started_ns = clock::monotonic_nanos();
                previous_ns = shader_started_ns;
                frame_number = 0;
                stats_started_ns = shader_started_ns;
                stats_frames = 0;
                stats_render_ns = 0;
                stats_max_render_ns = 0;
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
            flags: if native_resolution && SHADERS[shader_index].id == SHADERTOY_PROTEAN_CLOUDS {
                SHADERTOY_FLAG_NATIVE_RESOLUTION
            } else { 0 },
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
        let render_started_ns = clock::monotonic_nanos();
        if let Err(error) = frame.render_shadertoy(&params) {
            logl::log(logl::level::ERROR, format_args!(
                "shadertoy: failed frame shader={} extent={}x{} render_publish_ms={}",
                params.shader_id, width, height,
                clock::monotonic_nanos().saturating_sub(render_started_ns) / 1_000_000,
            ));
            fail("render/compute publish", error);
            return;
        }
        let rendered_ns = clock::monotonic_nanos();
        let render_ns = rendered_ns.saturating_sub(render_started_ns);
        stats_frames += 1;
        stats_render_ns = stats_render_ns.saturating_add(render_ns);
        stats_max_render_ns = stats_max_render_ns.max(render_ns);
        let stats_elapsed_ns = rendered_ns.saturating_sub(stats_started_ns);
        if stats_elapsed_ns >= 5_000_000_000 {
            logl::log(logl::level::INFO, format_args!(
                "shadertoy: perf shader={} extent={}x{} sampling={} frames={} fps_x100={} render_publish_us_avg={} render_publish_us_max={}",
                params.shader_id, width, height,
                if params.shader_id == SHADERTOY_PROTEAN_CLOUDS && !native_resolution { "radial-auto" } else { "native" },
                stats_frames,
                stats_frames.saturating_mul(100_000_000_000) / stats_elapsed_ns,
                stats_render_ns / stats_frames / 1_000,
                stats_max_render_ns / 1_000,
            ));
            stats_started_ns = rendered_ns;
            stats_frames = 0;
            stats_render_ns = 0;
            stats_max_render_ns = 0;
        }
        previous_ns = now_ns;
        frame_number = frame_number.wrapping_add(1);
    }
}

fn log_shader(shader: Shader) {
    logl::log(
        logl::level::INFO,
        format_args!(
            "shadertoy: selected '{}' id={} artifact_sha256={} controls=Left/Right,F1-F6,Space(clouds-focus/native),Esc",
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
