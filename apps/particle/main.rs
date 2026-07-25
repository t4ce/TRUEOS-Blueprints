#![no_std]

use trueos::ui4_scene::{
    Damage, Frame, PARTICLE_CRAFT_FLAG_ATTRACTOR, PARTICLE_CRAFT_FLAG_ORBIT,
    PARTICLE_CRAFT_FLAG_RESET, PARTICLE_CRAFT_HEIGHT, PARTICLE_CRAFT_WIDTH, ParticleCraftParamsV1,
};
use trueos::{clock, logl, vsys};

const FRAME_X: i32 = 640;
const FRAME_Y: i32 = 120;
const FRAME_MS: u64 = 33;
const POINTER_HOLD_MS: u64 = 2_000;
const SEED: u32 = 0xC0FF_EE51;

fn main() {
    logl::log(
        logl::level::INFO,
        "particle: opening ParticleCraft Arc Forge (C++/IGC, stateful two-pass)",
    );
    let Ok(mut frame) = Frame::open_streaming(
        FRAME_X,
        FRAME_Y,
        PARTICLE_CRAFT_WIDTH,
        PARTICLE_CRAFT_HEIGHT,
    ) else {
        logl::log(logl::level::ERROR, "particle: UI4 frame open failed");
        return;
    };

    let start_ms = clock::monotonic_millis();
    let mut last_ms = start_ms;
    let mut pointer_ms = 0u64;
    let mut attractor = (320.0f32, 180.0f32);
    let mut reset = true;
    let mut frame_width = PARTICLE_CRAFT_WIDTH;
    let mut frame_height = PARTICLE_CRAFT_HEIGHT;

    loop {
        vsys::poll_once();
        let now_ms = clock::monotonic_millis();
        let elapsed_ms = now_ms.saturating_sub(start_ms);
        let dt_ms = now_ms.saturating_sub(last_ms).clamp(1, 50);
        last_ms = now_ms;

        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => {
                    if event.width == frame_width && event.height == frame_height {
                        continue;
                    }
                    if let Err(error) = frame.resize(event.width, event.height) {
                        logl::log(
                            logl::level::ERROR,
                            format_args!("particle: resize failed: {error:?}"),
                        );
                        return;
                    }
                    frame_width = event.width;
                    frame_height = event.height;
                    logl::log(
                        logl::level::INFO,
                        format_args!(
                            "particle: resized {}x{} -> {}x{}",
                            event.old_width, event.old_height, frame_width, frame_height
                        ),
                    );
                }
                Ok(None) => break,
                Err(error) => {
                    logl::log(
                        logl::level::ERROR,
                        format_args!("particle: resize event failed: {error:?}"),
                    );
                    return;
                }
            }
        }

        loop {
            match frame.take_pointer_event() {
                Ok(Some(event)) => {
                    attractor.0 = (event.local_x as f32 * PARTICLE_CRAFT_WIDTH as f32
                        / frame_width as f32)
                        .clamp(0.0, PARTICLE_CRAFT_WIDTH as f32);
                    attractor.1 = (event.local_y as f32 * PARTICLE_CRAFT_HEIGHT as f32
                        / frame_height as f32)
                        .clamp(0.0, PARTICLE_CRAFT_HEIGHT as f32);
                    pointer_ms = now_ms;
                }
                Ok(None) => break,
                Err(error) => {
                    logl::log(
                        logl::level::ERROR,
                        format_args!("particle: pointer event failed: {error:?}"),
                    );
                    return;
                }
            }
        }

        let mut params = ParticleCraftParamsV1::arc_forge(
            elapsed_ms as f32 / 1_000.0,
            dt_ms as f32 / 1_000.0,
            SEED,
        );
        if reset {
            params.flags |= PARTICLE_CRAFT_FLAG_RESET;
            reset = false;
        }
        if pointer_ms != 0 && now_ms.saturating_sub(pointer_ms) <= POINTER_HOLD_MS {
            params.flags &= !PARTICLE_CRAFT_FLAG_ORBIT;
            params.flags |= PARTICLE_CRAFT_FLAG_ATTRACTOR;
            params.attractor_x = attractor.0;
            params.attractor_y = attractor.1;
            params.attraction = 126.0;
            params.swirl = 94.0;
        }

        let presented = frame
            .begin_gpu_frame()
            .and_then(|()| frame.render_particle_craft(&params))
            .and_then(|()| frame.publish(Damage::full(frame_width, frame_height)));
        if let Err(error) = presented {
            logl::log(
                logl::level::ERROR,
                format_args!("particle: ParticleCraft publish failed: {error:?}"),
            );
            return;
        }
        vsys::sleep_ms(FRAME_MS);
    }
}
