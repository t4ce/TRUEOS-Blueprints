#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, KeyboardState, rgba};
use trueos::{async_fs, env, vshell, vsys};
use trueos_gboi::{GameBoyButton, GameBoyEmulator};

const DEFAULT_ROM_PATH: &str = "common/gboi.gb";

const FRAME_SCALE: usize = 4;
const MAXIMIZED_FRAME_SCALE: usize = FRAME_SCALE * 3 / 2;
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
            "gboi: UI4 frame={}x{} buffers=2; click for keyboard focus; Esc exits",
            FRAME_WIDTH, FRAME_HEIGHT
        ),
    );
    shell_line("gboi: app:// is the private root; common/... is shared");
    shell_line("gboi: commands: list [app|common], load <path>, status, help");
    shell_prompt();

    let mut layout = DisplayLayout::new(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("the initial UI4 frame fits the native Game Boy display");
    let mut argb = vec![0u32; layout.content_pixel_count()];
    let mut rgba8 = vec![0u8; layout.frame_pixel_count() * 4];
    clear_rgba8(&mut rgba8);
    let mut frame_number = 0u64;
    let mut shell = ShellInput::new();

    loop {
        if let Err(error) = handle_resize_events(&mut frame, &mut layout, &mut argb, &mut rgba8) {
            logl::log(
                level::ERROR,
                format_args!("gboi: UI4 resize-event read failed error={error:?}"),
            );
            break;
        }

        if let Some(command) = shell.poll() {
            handle_shell_command(command.as_str(), &mut emulator, &mut current_rom);
            shell_prompt();
        }

        let keyboard = match frame.keyboard_state() {
            Ok(keyboard) => keyboard,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!("gboi: UI4 keyboard-state read failed error={error:?}"),
                );
                break;
            }
        };
        if key_is_down(keyboard.as_ref(), KEY_ESCAPE) {
            break;
        }
        sync_buttons(&mut emulator, keyboard.as_ref());

        emulator.tick();
        emulator.render(&mut argb, layout.content_width, layout.content_height);
        argb_to_centered_rgba8(&argb, &mut rgba8, layout);

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
    let listing = match async_fs::block_on(async_fs::list_dir(directory.as_bytes())) {
        Ok(listing) => listing,
        Err(error) => {
            shell_line(format!("gboi: list {label} failed ({error})").as_str());
            return;
        }
    };

    let mut entries = listing.entries;
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));

    shell_line(format!("gboi: ROMs in {label}").as_str());
    let mut found = 0usize;
    for entry in entries {
        if !matches!(entry.kind, async_fs::NodeKind::File) || !is_rom_name(entry.name.as_str()) {
            continue;
        }
        let path = if directory.is_empty() {
            entry.name
        } else {
            format!("{directory}/{}", entry.name)
        };
        let Ok(metadata) = async_fs::block_on(async_fs::metadata(path.as_bytes())) else {
            continue;
        };
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
    frame.publish(Damage::full(frame.width(), frame.height()))
}

#[derive(Clone, Copy)]
struct DisplayLayout {
    frame_width: usize,
    frame_height: usize,
    content_width: usize,
    content_height: usize,
    content_x: usize,
    content_y: usize,
    scale: usize,
}

impl DisplayLayout {
    fn new(frame_width: u32, frame_height: u32) -> Option<Self> {
        let frame_width = frame_width as usize;
        let frame_height = frame_height as usize;
        let fit_scale = (frame_width / trueos_gboi::gpu::SCREEN_W)
            .min(frame_height / trueos_gboi::gpu::SCREEN_H);
        if fit_scale == 0 {
            return None;
        }
        let maximized = frame_width > FRAME_WIDTH as usize || frame_height > FRAME_HEIGHT as usize;
        let requested_scale = if maximized {
            MAXIMIZED_FRAME_SCALE
        } else {
            FRAME_SCALE
        };
        let scale = requested_scale.min(fit_scale);
        let content_width = trueos_gboi::gpu::SCREEN_W * scale;
        let content_height = trueos_gboi::gpu::SCREEN_H * scale;
        Some(Self {
            frame_width,
            frame_height,
            content_width,
            content_height,
            content_x: (frame_width - content_width) / 2,
            content_y: (frame_height - content_height) / 2,
            scale,
        })
    }

    const fn frame_pixel_count(self) -> usize {
        self.frame_width * self.frame_height
    }

    const fn content_pixel_count(self) -> usize {
        self.content_width * self.content_height
    }
}

fn handle_resize_events(
    frame: &mut Frame,
    layout: &mut DisplayLayout,
    argb: &mut Vec<u32>,
    rgba8: &mut Vec<u8>,
) -> Result<(), Ui4Error> {
    while let Some(resize) = frame.take_resize_event()? {
        if resize.width == frame.width() && resize.height == frame.height() {
            continue;
        }
        let Some(next_layout) = DisplayLayout::new(resize.width, resize.height) else {
            logl::log(
                level::WARN,
                format_args!(
                    "gboi: ignored resize {}x{} -> {}x{}; native display does not fit",
                    resize.old_width, resize.old_height, resize.width, resize.height
                ),
            );
            continue;
        };
        if let Err(error) = frame.resize(resize.width, resize.height) {
            logl::log(
                level::WARN,
                format_args!(
                    "gboi: resize {}x{} -> {}x{} failed: {error:?}",
                    resize.old_width, resize.old_height, resize.width, resize.height
                ),
            );
            continue;
        }

        *layout = next_layout;
        argb.resize(layout.content_pixel_count(), 0);
        rgba8.resize(layout.frame_pixel_count() * 4, 0);
        clear_rgba8(rgba8);
        logl::log(
            level::INFO,
            format_args!(
                "gboi: resized {}x{} -> {}x{} content={}x{} scale={}x centered={},{}",
                resize.old_width,
                resize.old_height,
                resize.width,
                resize.height,
                layout.content_width,
                layout.content_height,
                layout.scale,
                layout.content_x,
                layout.content_y
            ),
        );
    }
    Ok(())
}

fn sync_buttons(emulator: &mut GameBoyEmulator, keyboard: Option<&KeyboardState>) {
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
                .any(|key_code| key_is_down(keyboard, *key_code)),
        );
    }
}

fn key_is_down(keyboard: Option<&KeyboardState>, key_code: u8) -> bool {
    keyboard.is_some_and(|keyboard| keyboard.is_down(key_code))
}

fn argb_to_centered_rgba8(source: &[u32], destination: &mut [u8], layout: DisplayLayout) {
    for source_y in 0..layout.content_height {
        let source_start = source_y * layout.content_width;
        let destination_start =
            ((layout.content_y + source_y) * layout.frame_width + layout.content_x) * 4;
        let source_row = &source[source_start..source_start + layout.content_width];
        let destination_row =
            &mut destination[destination_start..destination_start + layout.content_width * 4];
        for (pixel, rgba) in source_row.iter().zip(destination_row.chunks_exact_mut(4)) {
            rgba[0] = ((pixel >> 16) & 0xFF) as u8;
            rgba[1] = ((pixel >> 8) & 0xFF) as u8;
            rgba[2] = (pixel & 0xFF) as u8;
            rgba[3] = u8::MAX;
        }
    }
}

fn clear_rgba8(pixels: &mut [u8]) {
    pixels.fill(0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = u8::MAX;
    }
}
