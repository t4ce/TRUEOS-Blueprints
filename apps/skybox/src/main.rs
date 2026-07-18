#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use trueos::ui4_scene::{Damage, Frame, SkyboxRenderParams};
use trueos::{hid, logl, vshell, vsys};

include!(concat!(env!("OUT_DIR"), "/skybox_meta.rs"));

const SKYBOX_RGB565: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/skybox_rgb565.bin"));

const DEFAULT_FRAME_X: i32 = 0;
const DEFAULT_FRAME_Y: i32 = 0;
const DEFAULT_FRAME_WIDTH: u32 = 2560;
const DEFAULT_FRAME_HEIGHT: u32 = 1440;
const WINDOW_FRAME_X: i32 = 80;
const WINDOW_FRAME_Y: i32 = 72;
const WINDOW_FRAME_WIDTH: u32 = 1920;
const WINDOW_FRAME_HEIGHT: u32 = 1080;
const TEST_RIG_WIDTH: u32 = 2560;
const TEST_RIG_HEIGHT: u32 = 1440;
const MAX_FRAME_WIDTH: u32 = 2560;
const MAX_FRAME_HEIGHT: u32 = 1440;
const FOV_Y_DEGREES: f32 = 72.0;
const PI: f32 = 3.141_592_7;
const TAU: f32 = 6.283_185_5;
const INV_PI: f32 = 0.318_309_87;
const HALF_INV_PI: f32 = 0.159_154_94;

#[derive(Clone, Copy)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }

    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }

    fn normalized(self) -> Self {
        let len2 = self.x * self.x + self.y * self.y + self.z * self.z;
        if len2 <= 0.0 {
            return Self::new(0.0, 1.0, 0.0);
        }
        self.mul(1.0 / libm::sqrtf(len2))
    }
}

#[derive(Clone, Copy)]
struct View {
    yaw: f32,
    pitch: f32,
    target_yaw: f32,
    target_pitch: f32,
    cursor_seq: u64,
}

struct ShellInput {
    bytes: [u8; 128],
    len: usize,
}

impl ShellInput {
    const fn new() -> Self {
        Self {
            bytes: [0; 128],
            len: 0,
        }
    }

    fn poll(&mut self) -> Option<&str> {
        while let Some(byte) = vshell::attached_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    if self.len == 0 {
                        continue;
                    }
                    let len = self.len;
                    self.len = 0;
                    return core::str::from_utf8(&self.bytes[..len]).ok();
                }
                8 | 127 => {
                    self.len = self.len.saturating_sub(1);
                }
                0x20..=0x7e => {
                    if self.len < self.bytes.len() {
                        self.bytes[self.len] = byte;
                        self.len += 1;
                    }
                }
                _ => {}
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
struct Layout {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Layout {
    const fn default() -> Self {
        Self {
            x: DEFAULT_FRAME_X,
            y: DEFAULT_FRAME_Y,
            width: DEFAULT_FRAME_WIDTH,
            height: DEFAULT_FRAME_HEIGHT,
        }
    }

    const fn full_test_rig() -> Self {
        Self {
            x: 0,
            y: 0,
            width: TEST_RIG_WIDTH,
            height: TEST_RIG_HEIGHT,
        }
    }

    const fn window() -> Self {
        Self {
            x: WINDOW_FRAME_X,
            y: WINDOW_FRAME_Y,
            width: WINDOW_FRAME_WIDTH,
            height: WINDOW_FRAME_HEIGHT,
        }
    }

    fn clamp_size(&mut self) {
        self.width = self.width.clamp(64, MAX_FRAME_WIDTH);
        self.height = self.height.clamp(64, MAX_FRAME_HEIGHT);
    }
}

impl View {
    const fn new() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.08,
            target_yaw: 0.0,
            target_pitch: 0.08,
            cursor_seq: 0,
        }
    }

    fn update_from_cursor(&mut self) {
        if let Some((x, y)) = latest_cursor_position(&mut self.cursor_seq) {
            self.target_yaw = wrap_angle((clamp(x, 0.0, 1.0) - 0.5) * TAU);
            self.target_pitch = clamp((0.5 - clamp(y, 0.0, 1.0)) * PI * 0.86, -1.35, 1.35);
        }
        self.yaw = wrap_angle(self.yaw + shortest_angle_delta(self.yaw, self.target_yaw) * 0.28);
        self.pitch += (self.target_pitch - self.pitch) * 0.28;
    }
}

fn main() {
    status_line("skybox: boot");
    status_line("skybox: commands: full 1440p window size <w> <h> pos <x> <y> status help");
    let mut layout = Layout::default();
    logl::log(
        logl::level::INFO,
        format_args!(
            "skybox: starting FluidX3D skybox {}x{} sampled as {}x{} -> {}x{}",
            SKYBOX_SOURCE_WIDTH,
            SKYBOX_SOURCE_HEIGHT,
            SKYBOX_WIDTH,
            SKYBOX_HEIGHT,
            layout.width,
            layout.height
        ),
    );

    let mut rgba = rgba_buffer(layout.width, layout.height);
    let mut view = View::new();

    let Ok(mut frame) = Frame::open(layout.x, layout.y, layout.width, layout.height) else {
        status_line("skybox: frame create failed");
        logl::log(
            logl::level::ERROR,
            "skybox: failed to create ui4 scene frame",
        );
        return;
    };
    status_line("skybox: ui4 scene frame created");

    let mut gpu_ready = frame
        .upload_skybox_rgb565(SKYBOX_WIDTH as u32, SKYBOX_HEIGHT as u32, SKYBOX_RGB565)
        .is_ok();
    if gpu_ready {
        status_line("skybox: rgb565 skybox uploaded");
    } else {
        status_line("skybox: rgb565 upload failed, using cpu fallback");
    }

    if !present_skybox(
        &mut frame,
        layout.width,
        layout.height,
        view,
        &mut gpu_ready,
        &mut rgba,
    ) {
        status_line("skybox: initial ui4 publish failed");
        logl::log(logl::level::ERROR, "skybox: initial ui4 publish failed");
        return;
    }
    status_line("skybox: first ui4 frame presented");

    let mut rendered_yaw = view.yaw;
    let mut rendered_pitch = view.pitch;
    let mut shell = ShellInput::new();

    loop {
        let mut force_render = false;
        if let Some(command) = shell.poll() {
            match handle_command(command, &mut frame, &mut layout, &mut rgba) {
                CommandResult::Render => force_render = true,
                CommandResult::NoRender => {}
                CommandResult::Quit => return,
            }
        }

        view.update_from_cursor();
        let moved = shortest_angle_delta(rendered_yaw, view.yaw).abs() > 0.001
            || (rendered_pitch - view.pitch).abs() > 0.001;
        if moved || force_render {
            if !present_skybox(
                &mut frame,
                layout.width,
                layout.height,
                view,
                &mut gpu_ready,
                &mut rgba,
            ) {
                status_line("skybox: ui4 publish failed");
                logl::log(logl::level::ERROR, "skybox: ui4 publish failed");
                return;
            }
            rendered_yaw = view.yaw;
            rendered_pitch = view.pitch;
        }
        vsys::poll_once();
        vsys::sleep_ms(24);
    }
}

enum CommandResult {
    Render,
    NoRender,
    Quit,
}

fn handle_command(
    command: &str,
    frame: &mut Frame,
    layout: &mut Layout,
    rgba: &mut Vec<u8>,
) -> CommandResult {
    let mut parts = command.split_whitespace();
    let Some(op) = parts.next() else {
        return CommandResult::NoRender;
    };

    match op {
        "help" | "?" => {
            status_line("skybox commands:");
            status_line("  full | 1440p       set 2560x1440 at 0,0");
            status_line("  1080p              set 1920x1080 at 0,0");
            status_line("  720p               set 1280x720 at 0,0");
            status_line("  window             set 1920x1080 at 80,72");
            status_line("  size <w> <h>       resize, clamped to 2560x1440");
            status_line("  size <w>x<h>       resize shorthand");
            status_line("  pos <x> <y>        move frame");
            status_line("  status             print current frame");
            status_line("  quit               close skybox");
            CommandResult::NoRender
        }
        "status" => {
            status_line(
                format!(
                    "skybox: pos={}x{} size={}x{}",
                    layout.x, layout.y, layout.width, layout.height
                )
                .as_str(),
            );
            CommandResult::NoRender
        }
        "quit" | "exit" => {
            status_line("skybox: exit");
            CommandResult::Quit
        }
        "full" | "fullscreen" | "1440p" => {
            apply_layout(frame, layout, rgba, Layout::full_test_rig())
        }
        "1080p" => apply_layout(
            frame,
            layout,
            rgba,
            Layout {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ),
        "720p" => apply_layout(
            frame,
            layout,
            rgba,
            Layout {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
        ),
        "window" | "small" => apply_layout(frame, layout, rgba, Layout::window()),
        "size" | "resize" => {
            let Some((width, height)) = parse_size(parts.next(), parts.next()) else {
                status_line("skybox: usage size <w> <h> or size <w>x<h>");
                return CommandResult::NoRender;
            };
            apply_layout(
                frame,
                layout,
                rgba,
                Layout {
                    x: layout.x,
                    y: layout.y,
                    width,
                    height,
                },
            )
        }
        "pos" | "move" => {
            let Some(x) = parts.next().and_then(parse_i32) else {
                status_line("skybox: usage pos <x> <y>");
                return CommandResult::NoRender;
            };
            let Some(y) = parts.next().and_then(parse_i32) else {
                status_line("skybox: usage pos <x> <y>");
                return CommandResult::NoRender;
            };
            if frame.set_position(x, y).is_ok() {
                layout.x = x;
                layout.y = y;
                status_line(
                    format!(
                        "skybox: pos={}x{} size={}x{}",
                        layout.x, layout.y, layout.width, layout.height
                    )
                    .as_str(),
                );
            } else {
                status_line("skybox: position failed");
            }
            CommandResult::NoRender
        }
        _ => {
            status_line("skybox: unknown command, try help");
            CommandResult::NoRender
        }
    }
}

fn apply_layout(
    frame: &mut Frame,
    layout: &mut Layout,
    rgba: &mut Vec<u8>,
    mut requested: Layout,
) -> CommandResult {
    requested.clamp_size();
    let size_changed = layout.width != requested.width || layout.height != requested.height;
    let size_ok = !size_changed || frame.resize(requested.width, requested.height).is_ok();
    if !size_ok {
        status_line(
            format!(
                "skybox: resize failed requested={}x{} current={}x{}",
                requested.width, requested.height, layout.width, layout.height
            )
            .as_str(),
        );
        return CommandResult::NoRender;
    }

    let pos_changed = layout.x != requested.x || layout.y != requested.y;
    let pos_ok = !pos_changed || frame.set_position(requested.x, requested.y).is_ok();
    if !pos_ok {
        status_line("skybox: position failed after resize");
    }

    *layout = Layout {
        x: if pos_ok { requested.x } else { layout.x },
        y: if pos_ok { requested.y } else { layout.y },
        width: requested.width,
        height: requested.height,
    };
    rgba.resize(layout.width as usize * layout.height as usize * 4, 0);
    status_line(
        format!(
            "skybox: pos={}x{} size={}x{}",
            layout.x, layout.y, layout.width, layout.height
        )
        .as_str(),
    );
    CommandResult::Render
}

fn parse_size(first: Option<&str>, second: Option<&str>) -> Option<(u32, u32)> {
    if let Some(first) = first {
        if let Some((w, h)) = first.split_once('x').or_else(|| first.split_once('X')) {
            return Some((parse_u32(w)?, parse_u32(h)?));
        }
    }
    Some((parse_u32(first?)?, parse_u32(second?)?))
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok()
}

fn parse_i32(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

fn rgba_buffer(width: u32, height: u32) -> Vec<u8> {
    vec![0u8; width as usize * height as usize * 4]
}

fn status_line(text: &str) {
    vsys::write_out(text.as_bytes());
    vsys::write_out(b"\n");
}

fn present_skybox(
    frame: &mut Frame,
    width: u32,
    height: u32,
    view: View,
    gpu_ready: &mut bool,
    rgba: &mut [u8],
) -> bool {
    if frame.begin(trueos::ui4_scene::rgba(0, 0, 0, 255)).is_err() {
        return false;
    }

    if *gpu_ready {
        let params = skybox_render_params(width, height, view);
        if frame.render_skybox_rgb565(&params).is_ok() {
            return frame.publish(Damage::full(width, height)).is_ok();
        }
        status_line("skybox: gpu render failed, using cpu fallback");
        *gpu_ready = false;
    }

    render_skybox(rgba, width as usize, height as usize, view);
    frame.write_opaque_rgba8(rgba).is_ok() && frame.publish(Damage::full(width, height)).is_ok()
}

fn skybox_render_params(width: u32, height: u32, view: View) -> SkyboxRenderParams {
    let aspect = width as f32 / height as f32;
    let half_fov = 0.5 * FOV_Y_DEGREES * PI / 180.0;
    let tan_half = libm::tanf(half_fov);
    let sin_yaw = libm::sinf(view.yaw);
    let cos_yaw = libm::cosf(view.yaw);
    let sin_pitch = libm::sinf(view.pitch);
    let cos_pitch = libm::cosf(view.pitch);

    let forward = Vec3::new(sin_yaw * cos_pitch, cos_yaw * cos_pitch, sin_pitch);
    let right = Vec3::new(cos_yaw, -sin_yaw, 0.0);
    let up = Vec3::new(-sin_yaw * sin_pitch, -cos_yaw * sin_pitch, cos_pitch);

    SkyboxRenderParams {
        right_x: right.x,
        right_y: right.y,
        right_z: right.z,
        up_x: up.x,
        up_y: up.y,
        up_z: up.z,
        forward_x: forward.x,
        forward_y: forward.y,
        forward_z: forward.z,
        aspect_tan_half_fov_y: aspect * tan_half,
        tan_half_fov_y: tan_half,
        rect_x: 0,
        rect_y: 0,
        rect_width: width,
        rect_height: height,
    }
}

fn render_skybox(out: &mut [u8], width: usize, height: usize, view: View) {
    let aspect = width as f32 / height as f32;
    let half_fov = 0.5 * FOV_Y_DEGREES * PI / 180.0;
    let focal_y = libm::tanf(half_fov);
    let sin_yaw = libm::sinf(view.yaw);
    let cos_yaw = libm::cosf(view.yaw);
    let sin_pitch = libm::sinf(view.pitch);
    let cos_pitch = libm::cosf(view.pitch);

    let forward = Vec3::new(sin_yaw * cos_pitch, cos_yaw * cos_pitch, sin_pitch);
    let right = Vec3::new(cos_yaw, -sin_yaw, 0.0);
    let up = Vec3::new(-sin_yaw * sin_pitch, -cos_yaw * sin_pitch, cos_pitch);

    for y in 0..height {
        let camera_y = (1.0 - 2.0 * ((y as f32 + 0.5) / height as f32)) * focal_y;
        for x in 0..width {
            let camera_x = (2.0 * ((x as f32 + 0.5) / width as f32) - 1.0) * aspect * focal_y;
            let direction = forward
                .add(right.mul(camera_x))
                .add(up.mul(camera_y))
                .normalized();
            let [r, g, b] = sample_fluidx3d_skybox(direction);
            let offset = (x + y * width) * 4;
            out[offset] = r;
            out[offset + 1] = g;
            out[offset + 2] = b;
            out[offset + 3] = 255;
        }
    }
}

fn sample_fluidx3d_skybox(direction: Vec3) -> [u8; 3] {
    let direction = direction.normalized();
    let fu = SKYBOX_WIDTH as f32 * (libm::atan2f(direction.x, direction.y) * HALF_INV_PI + 0.5);
    let fv = SKYBOX_HEIGHT as f32 * (libm::asinf(clamp(direction.z, -1.0, 1.0)) * -INV_PI + 0.5);

    let ua = clamp(fu as i32 as f32, 0.0, (SKYBOX_WIDTH - 1) as f32) as usize;
    let va = clamp(fv as i32 as f32, 0.0, (SKYBOX_HEIGHT - 1) as f32) as usize;
    let ub = (ua + 1) % SKYBOX_WIDTH;
    let vb = (va + 1).min(SKYBOX_HEIGHT - 1);

    let u1 = fu - ua as f32;
    let v1 = fv - va as f32;
    let u0 = 1.0 - u1;
    let v0 = 1.0 - v1;

    let s00 = rgb565_at(ua, va);
    let s01 = rgb565_at(ua, vb);
    let s10 = rgb565_at(ub, va);
    let s11 = rgb565_at(ub, vb);

    color_mix(color_mix(s00, s01, v0), color_mix(s10, s11, v0), u0)
}

fn rgb565_at(x: usize, y: usize) -> [u8; 3] {
    let offset = (x + y * SKYBOX_WIDTH) * 2;
    if offset + 1 >= SKYBOX_RGB565.len() {
        return [0, 0, 0];
    }
    let value = u16::from_le_bytes([SKYBOX_RGB565[offset], SKYBOX_RGB565[offset + 1]]);
    let r5 = ((value >> 11) & 0x1f) as u8;
    let g6 = ((value >> 5) & 0x3f) as u8;
    let b5 = (value & 0x1f) as u8;
    [
        (r5 << 3) | (r5 >> 2),
        (g6 << 2) | (g6 >> 4),
        (b5 << 3) | (b5 >> 2),
    ]
}

fn color_mix(c1: [u8; 3], c2: [u8; 3], w: f32) -> [u8; 3] {
    let inverse = 1.0 - w;
    [
        clamp_u8(w * c1[0] as f32 + inverse * c2[0] as f32 + 0.5),
        clamp_u8(w * c1[1] as f32 + inverse * c2[1] as f32 + 0.5),
        clamp_u8(w * c1[2] as f32 + inverse * c2[2] as f32 + 0.5),
    ]
}

fn latest_cursor_position(cursor_seq: &mut u64) -> Option<(f32, f32)> {
    if let Ok((events, next_seq, _)) = hid::read_cursor_events_since(*cursor_seq, 32) {
        *cursor_seq = next_seq;
        if let Some(event) = events.last() {
            return Some((event.x as f32, event.y as f32));
        }
    }
    hid::hid_hut_mice()
        .into_iter()
        .next()
        .map(|mouse| (mouse.x as f32, mouse.y as f32))
}

fn shortest_angle_delta(from: f32, to: f32) -> f32 {
    let mut delta = wrap_angle(to - from);
    if delta > PI {
        delta -= TAU;
    }
    delta
}

fn wrap_angle(mut angle: f32) -> f32 {
    while angle < 0.0 {
        angle += TAU;
    }
    while angle >= TAU {
        angle -= TAU;
    }
    angle
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn clamp_u8(value: f32) -> u8 {
    clamp(value, 0.0, 255.0) as u8
}
