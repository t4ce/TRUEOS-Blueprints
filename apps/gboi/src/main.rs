#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, rgba};
use trueos::{env, hid, vfs, vsys};
use trueos_gboi::{GameBoyButton, GameBoyEmulator};

const DEFAULT_ROM_PATH: &str = "common/gboi.gb";

const FRAME_SCALE: usize = 4;
const FRAME_WIDTH: u32 = (trueos_gboi::gpu::SCREEN_W * FRAME_SCALE) as u32;
const FRAME_HEIGHT: u32 = (trueos_gboi::gpu::SCREEN_H * FRAME_SCALE) as u32;
const FRAME_X: i32 = 96;
const FRAME_Y: i32 = 80;
const FRAME_PERIOD_MS: u64 = 16;
const CLEAR_RGBA: u32 = rgba(0, 0, 0, 255);

const KEY_A: u8 = 0x04;
const KEY_C: u8 = 0x06;
const KEY_D: u8 = 0x07;
const KEY_S: u8 = 0x16;
const KEY_W: u8 = 0x1A;
const KEY_X: u8 = 0x1B;
const KEY_Z: u8 = 0x1D;
const KEY_ENTER: u8 = 0x28;
const KEY_ESCAPE: u8 = 0x29;
const KEY_SPACE: u8 = 0x2C;
const KEY_ARROW_RIGHT: u8 = 0x4F;
const KEY_ARROW_LEFT: u8 = 0x50;
const KEY_ARROW_DOWN: u8 = 0x51;
const KEY_ARROW_UP: u8 = 0x52;

fn main() {
    let rom_path = rom_path_from_args();
    let rom = match vfs::read_file(rom_path.as_bytes()) {
        Ok(rom) => rom,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!(
                    "gboi: ROM read failed path={} error={}; launch with a path or install {}",
                    rom_path, error, DEFAULT_ROM_PATH
                ),
            );
            return;
        }
    };

    let mut emulator = GameBoyEmulator::new();
    if !emulator.load_rom(rom.as_slice()) {
        logl::log(
            level::ERROR,
            format_args!(
                "gboi: ROM parser rejected {} bytes from {}",
                rom.len(),
                rom_path
            ),
        );
        return;
    }

    // `Frame::open` is intentionally the dirty/double-buffered UI4 request.
    // The streaming constructor would select the triple-buffer scene path.
    let Ok(mut frame) = Frame::open(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(level::ERROR, "gboi: UI4 double-buffer frame create failed");
        return;
    };

    logl::log(
        level::INFO,
        format_args!(
            "gboi: loaded {} bytes from {}; UI4 frame={}x{} buffers=2; Esc exits",
            rom.len(),
            rom_path,
            FRAME_WIDTH,
            FRAME_HEIGHT
        ),
    );

    let pixel_count = FRAME_WIDTH as usize * FRAME_HEIGHT as usize;
    let mut argb = vec![0u32; pixel_count];
    let mut rgba8 = vec![0u8; pixel_count * 4];
    let mut frame_number = 0u64;

    loop {
        let keyboards = hid::hid_hut_keyboards();
        if key_is_down(&keyboards, KEY_ESCAPE) {
            break;
        }
        sync_buttons(&mut emulator, &keyboards);

        emulator.tick();
        emulator.render(&mut argb, FRAME_WIDTH as usize, FRAME_HEIGHT as usize);
        argb_to_rgba8(&argb, &mut rgba8);

        if let Err(error) = present_frame(&mut frame, rgba8.as_slice()) {
            logl::log(
                level::ERROR,
                format_args!(
                    "gboi: UI4 frame publish failed frame={} error={error:?}",
                    frame_number
                ),
            );
            break;
        }

        frame_number = frame_number.wrapping_add(1);
        if frame_number <= 3 || frame_number.is_multiple_of(120) {
            logl::log(
                level::INFO,
                format_args!(
                    "gboi: published frame={} lifecycle=ui4-double",
                    frame_number
                ),
            );
        }

        vsys::poll_once();
        vsys::sleep_ms(FRAME_PERIOD_MS);
    }

    // Dropping `Frame` closes the broker window and retires both UI4 buffers.
    logl::log(
        level::INFO,
        format_args!("gboi: closing after {} frame(s)", frame_number),
    );
}

fn rom_path_from_args() -> String {
    let mut args = env::args();
    let _archive = args.next();
    args.next().unwrap_or_else(|| DEFAULT_ROM_PATH.to_string())
}

fn present_frame(frame: &mut Frame, rgba8: &[u8]) -> Result<(), Ui4Error> {
    loop {
        match frame.begin(CLEAR_RGBA) {
            Ok(()) => break,
            Err(Ui4Error::Busy) => {
                // With two buffers, SURFLIVE/read-lease retirement is the
                // backpressure boundary. Never overwrite or skip around it.
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
    frame.write_opaque_rgba8(rgba8)?;
    frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT))
}

fn sync_buttons(emulator: &mut GameBoyEmulator, keyboards: &[hid::TrueosHidHutKeyboardState]) {
    let mappings: &[(GameBoyButton, &[u8])] = &[
        (GameBoyButton::Right, &[KEY_D, KEY_ARROW_RIGHT]),
        (GameBoyButton::Left, &[KEY_A, KEY_ARROW_LEFT]),
        (GameBoyButton::Up, &[KEY_W, KEY_ARROW_UP]),
        (GameBoyButton::Down, &[KEY_S, KEY_ARROW_DOWN]),
        (GameBoyButton::A, &[KEY_X, KEY_SPACE]),
        (GameBoyButton::B, &[KEY_Z]),
        (GameBoyButton::Select, &[KEY_C]),
        (GameBoyButton::Start, &[KEY_ENTER]),
    ];

    for &(button, key_codes) in mappings {
        emulator.set_button(
            button,
            key_codes
                .iter()
                .any(|key_code| key_is_down(keyboards, *key_code)),
        );
    }
}

fn key_is_down(keyboards: &[hid::TrueosHidHutKeyboardState], key_code: u8) -> bool {
    let key = key_code as usize;
    keyboards
        .iter()
        .any(|keyboard| keyboard.key_down_bits[key / 32] & (1u32 << (key % 32)) != 0)
}

fn argb_to_rgba8(source: &[u32], destination: &mut [u8]) {
    for (pixel, rgba) in source.iter().zip(destination.chunks_exact_mut(4)) {
        rgba[0] = ((pixel >> 16) & 0xFF) as u8;
        rgba[1] = ((pixel >> 8) & 0xFF) as u8;
        rgba[2] = (pixel & 0xFF) as u8;
        rgba[3] = u8::MAX;
    }
}
