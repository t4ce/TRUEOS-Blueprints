#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, rgba};
use trueos::{async_fs, env, hid, vshell, vsys};
use trueos_gboi::{GameBoyButton, GameBoyEmulator};

const DEFAULT_ROM_PATH: &str = "common/gboi.gb";

const FRAME_SCALE: usize = 4;
const FRAME_WIDTH: u32 = (trueos_gboi::gpu::SCREEN_W * FRAME_SCALE) as u32;
const FRAME_HEIGHT: u32 = (trueos_gboi::gpu::SCREEN_H * FRAME_SCALE) as u32;
const FRAME_X: i32 = 96;
const FRAME_Y: i32 = 80;
const FRAME_PERIOD_MS: u64 = 16;
const CLEAR_RGBA: u32 = rgba(0, 0, 0, 255);
const SHELL_INPUT_CAP: usize = 512;

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
    let mut emulator = GameBoyEmulator::new();
    let mut current_rom = None;
    match load_rom_from_path(&mut emulator, rom_path.as_str()) {
        Ok(bytes) => {
            current_rom = Some(rom_path.clone());
            logl::log(
                level::INFO,
                format_args!("gboi: loaded {} bytes from {}", bytes, rom_path),
            );
        }
        Err(error) => {
            logl::log(
                level::WARN,
                format_args!(
                    "gboi: startup ROM unavailable path={} error={}; use `list` and `load <path>`",
                    rom_path, error
                ),
            );
        }
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
            "gboi: UI4 frame={}x{} buffers=2; Esc exits; shell commands are independent",
            FRAME_WIDTH, FRAME_HEIGHT
        ),
    );
    shell_line("gboi: app:// is the private root; common/... is shared");
    shell_line("gboi: commands: list [app|common], load <path>, status, help");
    shell_prompt();

    let pixel_count = FRAME_WIDTH as usize * FRAME_HEIGHT as usize;
    let mut argb = vec![0u32; pixel_count];
    let mut rgba8 = vec![0u8; pixel_count * 4];
    let mut frame_number = 0u64;
    let mut shell = ShellInput::new();

    loop {
        if let Some(command) = shell.poll() {
            handle_shell_command(command.as_str(), &mut emulator, &mut current_rom);
            shell_prompt();
        }

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
                    vshell::attached_write(b"\r\n");
                    let len = self.len;
                    self.len = 0;
                    return String::from_utf8(self.bytes[..len].to_vec()).ok();
                }
                8 | 127 => {
                    if self.len != 0 {
                        self.len -= 1;
                        vshell::attached_write(b"\x08 \x08");
                    }
                }
                0x20..=0x7e => {
                    if self.len < self.bytes.len() {
                        self.bytes[self.len] = byte;
                        self.len += 1;
                        vshell::attached_write(&[byte]);
                    }
                }
                _ => {}
            }
        }
        None
    }
}

fn shell_line(text: &str) {
    vshell::attached_write(text.as_bytes());
    vshell::attached_write(b"\r\n");
}

fn shell_prompt() {
    vshell::attached_write(b"gboi> ");
}

fn load_rom_from_path(emulator: &mut GameBoyEmulator, path: &str) -> Result<usize, String> {
    let rom = async_fs::block_on(async_fs::read_file(path.as_bytes()))
        .map_err(|error| format!("read failed ({error})"))?;
    let mut candidate = GameBoyEmulator::new();
    if !candidate.load_rom(rom.as_slice()) {
        return Err(format!("parser rejected {} bytes", rom.len()));
    }
    *emulator = candidate;
    Ok(rom.len())
}

fn list_roms(directory: &str) {
    let label = if directory.is_empty() {
        "app://"
    } else {
        "common://"
    };
    let listing = match async_fs::block_on(async_fs::list_dir_utf8(directory.as_bytes())) {
        Ok(listing) => listing,
        Err(error) => {
            shell_line(format!("gboi: list {label} failed ({error})").as_str());
            return;
        }
    };

    let mut names = listing
        .lines()
        .filter(|name| !name.is_empty() && *name != "...")
        .collect::<Vec<_>>();
    names.sort_unstable();

    shell_line(format!("gboi: ROMs in {label}").as_str());
    let mut found = 0usize;
    for name in names {
        if !is_rom_name(name) {
            continue;
        }
        let path = if directory.is_empty() {
            name.to_string()
        } else {
            format!("{directory}/{name}")
        };
        let Ok(metadata) = async_fs::block_on(async_fs::metadata(path.as_bytes())) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        found += 1;
        shell_line(format!("  {path} ({} bytes)", metadata.len).as_str());
    }
    if found == 0 {
        shell_line("  no .gb or .gbc files found");
    }
}

fn is_rom_name(name: &str) -> bool {
    name.ends_with(".gb")
        || name.ends_with(".gbc")
        || name.ends_with(".GB")
        || name.ends_with(".GBC")
}

fn handle_shell_command(
    command: &str,
    emulator: &mut GameBoyEmulator,
    current_rom: &mut Option<String>,
) {
    let command = command.trim();
    let mut parts = command.split_whitespace();
    let Some(operation) = parts.next() else {
        return;
    };

    match operation {
        "help" | "?" => {
            shell_line("gboi commands:");
            shell_line("  list             list ROMs in app://");
            shell_line("  list common      list ROMs in common://");
            shell_line("  load <path>      load app-relative or common/... ROM");
            shell_line("  status           show the active ROM");
        }
        "list" => match parts.next() {
            None | Some("app") | Some(".") => list_roms(""),
            Some("common") => list_roms("common"),
            Some(_) => shell_line("usage: list [app|common]"),
        },
        "load" => {
            let path = command[operation.len()..].trim();
            if path.is_empty() {
                shell_line("usage: load <path>");
                return;
            }
            match load_rom_from_path(emulator, path) {
                Ok(bytes) => {
                    *current_rom = Some(path.to_string());
                    shell_line(format!("gboi: loaded {path} ({bytes} bytes)").as_str());
                    logl::log(
                        level::INFO,
                        format_args!("gboi: shell loaded {} bytes from {}", bytes, path),
                    );
                }
                Err(error) => {
                    shell_line(format!("gboi: load {path} failed: {error}").as_str());
                }
            }
        }
        "status" => match current_rom.as_deref() {
            Some(path) => shell_line(format!("gboi: active ROM {path}").as_str()),
            None => shell_line("gboi: no ROM loaded"),
        },
        _ => shell_line("gboi: unknown command; use `help`"),
    }
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
