use anyhow::{Result, anyhow};
use trueos::{
    clock, input,
    ui4_scene::{self, Damage, Error, Frame},
    vsys,
};

use crate::{Controls, scene::Scene};

const FRAME_NS: u64 = 1_000_000_000 / 45;
const KEY_A: u8 = 0x04;
const KEY_D: u8 = 0x07;

pub(crate) fn run(width: u32, height: u32, controls: Controls, limit: Option<usize>) -> Result<()> {
    match present(width, height, controls, limit) {
        Ok(()) | Err(Error::NotFound) => Ok(()),
        Err(error) => Err(anyhow!("Hair UI4 frame: {error:?}")),
    }
}

fn present(
    width: u32,
    height: u32,
    mut controls: Controls,
    limit: Option<usize>,
) -> Result<(), Error> {
    let (screen_width, screen_height) = ui4_scene::output_dimensions()?;
    let (width, height) =
        ui4_scene::fit_frame_extent(width.min(screen_width), height.min(screen_height))?;
    let x = ((screen_width - width) / 2) as i32;
    let y = ((screen_height - height) / 2) as i32;
    let mut frame = Frame::open_streaming(x, y, width, height)?;
    frame.set_title("Hair · Flow study")?;
    let mut scene = Scene::new(width, height, controls.roll).map_err(|error| {
        eprintln!("hair: {error:#}");
        Error::Invalid
    })?;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let mut pending_resize = None;
    let mut shown = 0;
    let mut previous_frame = clock::monotonic_nanos();
    while limit.is_none_or(|count| shown < count) {
        let started = clock::monotonic_nanos();
        let elapsed = started.saturating_sub(previous_frame).min(100_000_000) as f32 * 1e-9;
        previous_frame = started;
        while let Some(event) = frame.take_resize_event()? {
            pending_resize = Some((event.width, event.height));
        }
        if let Some((width, height)) = pending_resize {
            match frame.resize(width, height) {
                Ok(()) => {
                    scene.resize(width, height);
                    pixels.resize(width as usize * height as usize * 4, 0);
                    pending_resize = None;
                }
                Err(Error::Busy) => (),
                Err(error) => return Err(error),
            }
        }
        while let Some(event) = frame.take_keyboard_event()? {
            if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                match char::from_u32(event.codepoint) {
                    Some('1') => controls.layers.avatar = !controls.layers.avatar,
                    Some('2') => controls.layers.hair = !controls.layers.hair,
                    Some('3') => controls.layers.sphere = !controls.layers.sphere,
                    Some('4') => {
                        controls.layers.guides = !controls.layers.guides;
                        controls.layers.field = controls.layers.guides;
                    }
                    Some('5') => controls.speed_color = !controls.speed_color,
                    Some('6') => controls.layers.reach = !controls.layers.reach,
                    Some('7') => controls.layers.body_obstacles = !controls.layers.body_obstacles,
                    Some('8') => controls.tip_spread = !controls.tip_spread,
                    _ => (),
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
            {
                match event.key_code {
                    input::KEYBOARD_KEY_SPACE => controls.paused = !controls.paused,
                    input::KEYBOARD_KEY_ARROW_LEFT => controls.yaw -= 0.06,
                    input::KEYBOARD_KEY_ARROW_RIGHT => controls.yaw += 0.06,
                    input::KEYBOARD_KEY_ESCAPE => return Ok(()),
                    _ => (),
                }
            }
        }
        if let Some(keyboard) = frame.keyboard_state()? {
            let direction = i8::from(keyboard.is_down(KEY_D)) - i8::from(keyboard.is_down(KEY_A));
            controls.roll_by(f32::from(direction), elapsed);
        }
        match frame.begin(ui4_scene::rgba(229, 235, 239, 255)) {
            Ok(()) => (),
            Err(Error::Busy) => {
                vsys::sleep_ms(1);
                continue;
            }
            Err(error) => return Err(error),
        }
        if !controls.paused {
            scene.advance(controls.roll, controls.tip_spread);
        }
        scene.draw(controls, &mut pixels);
        frame.write_opaque_rgba8(&pixels)?;
        let damage = Damage::full(frame.width(), frame.height());
        loop {
            match frame.publish(damage) {
                Ok(()) => break,
                Err(Error::Busy) => vsys::sleep_ms(1),
                Err(error) => return Err(error),
            }
        }
        shown += 1;
        let remaining = FRAME_NS.saturating_sub(clock::monotonic_nanos() - started);
        if remaining > 0 {
            vsys::sleep_ms(remaining.div_ceil(1_000_000));
        }
    }
    Ok(())
}
