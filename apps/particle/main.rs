#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use trueos::globalog::{self, level};
use trueos::platform;
use trueos::ui2::{self, gfx};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

const UI2_PARTICLE_DEMO_TEX_ID: u32 = 4_709;
const UI2_PARTICLE_DEMO_SPRITE_TEX_ID: u32 = 4_711;
const UI2_PARTICLE_DEMO_RT_W: u32 = 512;
const UI2_PARTICLE_DEMO_RT_H: u32 = 320;
const UI2_PARTICLE_DEMO_WINDOW_X: i32 = 640;
const UI2_PARTICLE_DEMO_WINDOW_Y: i32 = 120;
const UI2_PARTICLE_DEMO_WINDOW_Z: i32 = 34;
const UI2_PARTICLE_DEMO_WINDOW_ALPHA: u8 = 255;
const UI2_PARTICLE_DEMO_MAX_PARTICLES: usize = 96;
const UI2_PARTICLE_DEMO_FRAME_MS: u64 = 20;
const UI2_PARTICLE_DEMO_CLEAR_RGB: u32 = 0x070B11;
const UI2_PARTICLE_DEMO_SPRITE_SCALE: f32 = 15.0;
const UI2_PARTICLE_DEMO_SVG_SRC: &str = include_str!("parapath.svg");

#[derive(Clone, Copy, Debug, Default)]
struct ParticleSnapshot {
    x: f32,
    y: f32,
    size_px: f32,
    color_rgba: u32,
}

struct ParticleSystem {
    pos_x: Vec<f32>,
    pos_y: Vec<f32>,
    vel_x: Vec<f32>,
    vel_y: Vec<f32>,
    life: Vec<f32>,
    size_px: Vec<f32>,
    color_rgba: Vec<u32>,
    dead: Vec<u8>,
    alive_count: usize,
    max_count: usize,
}

impl ParticleSystem {
    fn new(max_count: usize) -> Self {
        Self {
            pos_x: vec![0.0; max_count],
            pos_y: vec![0.0; max_count],
            vel_x: vec![0.0; max_count],
            vel_y: vec![0.0; max_count],
            life: vec![0.0; max_count],
            size_px: vec![1.0; max_count],
            color_rgba: vec![0xFFFF_FFFF; max_count],
            dead: vec![0; max_count],
            alive_count: 0,
            max_count,
        }
    }

    fn alive_count(&self) -> usize {
        self.alive_count
    }

    fn spawn_styled(
        &mut self,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        life: f32,
        size_px: f32,
        color_rgba: u32,
    ) {
        if self.alive_count >= self.max_count {
            return;
        }
        let i = self.alive_count;
        self.pos_x[i] = x;
        self.pos_y[i] = y;
        self.vel_x[i] = vx;
        self.vel_y[i] = vy;
        self.life[i] = life;
        self.size_px[i] = size_px;
        self.color_rgba[i] = color_rgba;
        self.dead[i] = 0;
        self.alive_count += 1;
    }

    fn update_single_threaded(&mut self, dt: f32) {
        for i in 0..self.alive_count {
            self.life[i] -= dt;
            if self.life[i] <= 0.0 {
                self.dead[i] = 1;
                continue;
            }
            self.vel_y[i] += 8.0 * dt;
            self.vel_x[i] *= 0.998;
            self.vel_y[i] *= 0.998;
            self.pos_x[i] += self.vel_x[i] * dt;
            self.pos_y[i] += self.vel_y[i] * dt;
            self.size_px[i] *= 0.997;
        }
        self.compact_dead();
    }

    fn snapshot_into(&self, out: &mut Vec<ParticleSnapshot>) {
        out.clear();
        out.reserve(self.alive_count);
        for i in 0..self.alive_count {
            out.push(ParticleSnapshot {
                x: self.pos_x[i],
                y: self.pos_y[i],
                size_px: self.size_px[i],
                color_rgba: self.color_rgba[i],
            });
        }
    }

    fn compact_dead(&mut self) {
        let mut write = 0usize;
        for read in 0..self.alive_count {
            if self.dead[read] != 0 {
                continue;
            }
            if write != read {
                self.pos_x[write] = self.pos_x[read];
                self.pos_y[write] = self.pos_y[read];
                self.vel_x[write] = self.vel_x[read];
                self.vel_y[write] = self.vel_y[read];
                self.life[write] = self.life[read];
                self.size_px[write] = self.size_px[read];
                self.color_rgba[write] = self.color_rgba[read];
                self.dead[write] = 0;
            }
            write += 1;
        }
        self.alive_count = write;
    }
}

struct DemoRng(u64);

impl DemoRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

fn seed_particles(system: &mut ParticleSystem, width: u32, height: u32) {
    let mut rng = DemoRng::new(0xC0FFEE_1000);
    let width_f = width as f32;
    let height_f = height as f32;
    let cx = width_f * 0.5;
    let cy = height_f * 0.5;

    for _ in 0..UI2_PARTICLE_DEMO_MAX_PARTICLES {
        let angle = rng.range(0.0, core::f32::consts::TAU);
        let speed = rng.range(10.0, 64.0);
        let drift = rng.range(-8.0, 8.0);
        let vx = libm::cosf(angle) * speed + drift;
        let vy = libm::sinf(angle) * speed + drift * 0.35;
        let life = rng.range(1.5, 6.0);
        let size_px = rng.range(3.5, 8.0);
        let color = pack_rgba(
            0x90u8.saturating_add((rng.next_u32() & 0x3F) as u8),
            0x70u8.saturating_add((rng.next_u32() & 0x5F) as u8),
            0xA0u8.saturating_add((rng.next_u32() & 0x5F) as u8),
            0xC0u8.saturating_add((rng.next_u32() & 0x3F) as u8),
        );
        system.spawn_styled(cx, cy, vx, vy, life, size_px, color);
    }
}

fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32)
}

fn extract_svg_attr<'a>(svg: &'a str, attr: &str) -> Option<&'a str> {
    let start = svg.find(attr)? + attr.len();
    let tail = &svg[start..];
    let end = tail.find('"')?;
    Some(&tail[..end])
}

fn extract_svg_path_data(svg: &str, ordinal: usize) -> Option<&str> {
    let mut from = 0usize;
    for idx in 0..=ordinal {
        let rel = svg[from..].find(" d=\"")?;
        from += rel + 4;
        let tail = &svg[from..];
        let end = tail.find('"')?;
        if idx == ordinal {
            return Some(&tail[..end]);
        }
        from += end + 1;
    }
    None
}

fn normalized_particle_svg() -> String {
    let view_box = extract_svg_attr(UI2_PARTICLE_DEMO_SVG_SRC, "viewBox=\"")
        .unwrap_or("0 0 241.3044 506.9858");
    let outer = extract_svg_path_data(UI2_PARTICLE_DEMO_SVG_SRC, 0).unwrap_or("");
    let inner = extract_svg_path_data(UI2_PARTICLE_DEMO_SVG_SRC, 1).unwrap_or("");
    let mut d = String::with_capacity(outer.len() + inner.len() + 2);
    d.push_str(outer);
    d.push(' ');
    d.push_str(inner);

    let mut svg = String::with_capacity(d.len() + 128);
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"");
    svg.push_str(view_box);
    svg.push_str("\"><path fill=\"white\" fill-rule=\"evenodd\" stroke=\"none\" d=\"");
    svg.push_str(d.as_str());
    svg.push_str("\"/></svg>");
    svg
}

fn build_particle_verts(
    snapshot: &[ParticleSnapshot],
    width: u32,
    height: u32,
    sprite_size: (u32, u32),
) -> Vec<u8> {
    let transform = ViewTransform::from_extent(width, height);
    let mut verts = Vec::with_capacity(snapshot.len().saturating_mul(6 * TEX_VERTEX_SIZE));
    let sprite_aspect = if sprite_size.1 == 0 {
        1.0
    } else {
        sprite_size.0 as f32 / sprite_size.1 as f32
    };

    for particle in snapshot.iter().copied() {
        let sprite_h = particle.size_px.max(1.0) * UI2_PARTICLE_DEMO_SPRITE_SCALE;
        let sprite_w = sprite_h * sprite_aspect.max(0.05);
        let half_w = sprite_w * 0.5;
        let half_h = sprite_h * 0.5;
        push_tex_quad_px(
            &mut verts,
            transform,
            particle.x - half_w,
            particle.y - half_h,
            particle.x + half_w,
            particle.y + half_h,
            [0.0, 0.0, 1.0, 1.0],
            Rgba8::from_rgba_u32(particle.color_rgba),
        );
    }

    verts
}

fn respawn_dead_particles(system: &mut ParticleSystem, width: u32, height: u32, rng: &mut DemoRng) {
    let width_f = width as f32;
    let height_f = height as f32;
    let cx = width_f * 0.5;
    let cy = height_f * 0.55;

    while system.alive_count() < UI2_PARTICLE_DEMO_MAX_PARTICLES {
        let angle = rng.range(-1.4, -1.7);
        let spread = rng.range(-0.55, 0.55);
        let speed = rng.range(18.0, 92.0);
        let vx = libm::cosf(angle + spread) * speed;
        let vy = libm::sinf(angle + spread) * speed - rng.range(4.0, 24.0);
        let life = rng.range(1.0, 4.0);
        let size_px = rng.range(3.5, 9.0);
        let color = pack_rgba(
            0xC8u8.saturating_add((rng.next_u32() & 0x27) as u8),
            0x90u8.saturating_add((rng.next_u32() & 0x4F) as u8),
            0x48u8.saturating_add((rng.next_u32() & 0x67) as u8),
            0xA0u8.saturating_add((rng.next_u32() & 0x4F) as u8),
        );
        system.spawn_styled(cx, cy, vx, vy, life, size_px, color);
    }
}

fn create_particle_demo_window() -> Option<ui2::SurfaceWindow> {
    ui2::SurfaceWindow::create_with_options(
        "Particle System",
        ui2::Rect {
            x: UI2_PARTICLE_DEMO_WINDOW_X,
            y: UI2_PARTICLE_DEMO_WINDOW_Y,
            width: UI2_PARTICLE_DEMO_RT_W,
            height: UI2_PARTICLE_DEMO_RT_H,
        },
        ui2::CreateOptions {
            z: UI2_PARTICLE_DEMO_WINDOW_Z,
            alpha: UI2_PARTICLE_DEMO_WINDOW_ALPHA,
        },
        UI2_PARTICLE_DEMO_TEX_ID,
        false,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(surface) = create_particle_demo_window() else {
        globalog::log_with_level(level::ERROR, "particle bp: ui2 surface window create failed\n");
        return;
    };

    let mut system = ParticleSystem::new(UI2_PARTICLE_DEMO_MAX_PARTICLES);
    let mut snapshot = Vec::with_capacity(UI2_PARTICLE_DEMO_MAX_PARTICLES);
    let mut rng = DemoRng::new(0x51D3_1000);
    let sprite_svg = normalized_particle_svg();
    if gfx::upload_svg_to_texture(UI2_PARTICLE_DEMO_SPRITE_TEX_ID, sprite_svg.as_bytes()) != 0 {
        globalog::log_with_level(level::ERROR, "particle bp: svg upload failed\n");
        return;
    }
    let sprite_size = gfx::texture_dimensions(UI2_PARTICLE_DEMO_SPRITE_TEX_ID).unwrap_or((1, 1));

    seed_particles(&mut system, UI2_PARTICLE_DEMO_RT_W, UI2_PARTICLE_DEMO_RT_H);
    let _ = surface.id().set_title("Particle System");

    loop {
        system.update_single_threaded(UI2_PARTICLE_DEMO_FRAME_MS as f32 / 1000.0);
        respawn_dead_particles(
            &mut system,
            UI2_PARTICLE_DEMO_RT_W,
            UI2_PARTICLE_DEMO_RT_H,
            &mut rng,
        );
        system.snapshot_into(&mut snapshot);
        let verts = build_particle_verts(
            snapshot.as_slice(),
            UI2_PARTICLE_DEMO_RT_W,
            UI2_PARTICLE_DEMO_RT_H,
            sprite_size,
        );
        if !surface.render_tex_triangles(
            UI2_PARTICLE_DEMO_SPRITE_TEX_ID,
            UI2_PARTICLE_DEMO_CLEAR_RGB,
            verts.as_slice(),
        ) {
            globalog::log_with_level(level::ERROR, "particle bp: render queue failed\n");
            break;
        }
        platform::sleep_ms(UI2_PARTICLE_DEMO_FRAME_MS);
    }
}
