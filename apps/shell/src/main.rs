#![no_std]

use trueos::ui4_scene::{Damage, Frame, rgba};
use trueos::{logl, vsys};

const FRAME_X: i32 = 0;
const FRAME_Y: i32 = 0;
const FRAME_WIDTH: u32 = 1_024;
const FRAME_HEIGHT: u32 = 576;
const BASE_COLOR: u32 = rgba(8, 12, 20, 255);
const IDLE_INTERVAL_MS: u64 = 250;

const _: () = assert!(FRAME_WIDTH * 9 == FRAME_HEIGHT * 16);

fn main() {
    let Ok(mut frame) = Frame::open_immutable(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(logl::level::ERROR, "shell: UI4 frame reservation failed");
        return;
    };

    if let Err(error) = frame
        .begin(BASE_COLOR)
        .and_then(|()| frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT)))
    {
        logl::log(
            logl::level::ERROR,
            format_args!("shell: UI4 base frame publish failed: {error:?}"),
        );
        return;
    }

    logl::log(
        logl::level::INFO,
        "shell: reserved one immutable 1024x576 UI4 frame",
    );

    loop {
        vsys::poll_once();
        vsys::sleep_ms(IDLE_INTERVAL_MS);
    }
}
