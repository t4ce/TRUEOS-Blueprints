#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::array;

use trueos::ui4_scene::{Damage, Error, Frame, rgba};
use trueos::ui4_solara_text::{Font, MAX_SCENE_TEXT_ROWS_PER_CALL, SceneTextRow};
use trueos::{logl, vsys};

const FRAME_X: i32 = 640;
const FRAME_Y: i32 = 120;
const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 400;
const FRAME_MS: u64 = 33;
const PARTICLE_COUNT: usize = 48;
const HUD_ROW_COUNT: usize = 2;
const GLYPH: &str = "§";
const GRAVITY: f32 = 118.0;
const BACKGROUND: u32 = rgba(7, 11, 17, 255);
const COLORS: [u32; 4] = [
    rgba(255, 201, 103, 255),
    rgba(255, 126, 95, 255),
    rgba(180, 132, 255, 255),
    rgba(102, 214, 255, 255),
];

const _: () = assert!(PARTICLE_COUNT + HUD_ROW_COUNT <= MAX_SCENE_TEXT_ROWS_PER_CALL);

#[derive(Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
    age: f32,
    lifetime: f32,
    size: f32,
    color: usize,
}

impl Particle {
    fn font_pixels(self) -> f32 {
        let remaining = (1.0 - self.age / self.lifetime).clamp(0.0, 1.0);
        self.size * (0.58 + remaining * 0.42)
    }

    fn scene_row(self) -> SceneTextRow<'static> {
        let pixels = self.font_pixels();
        SceneTextRow {
            text: GLYPH,
            x: self.position.x - pixels * 0.24,
            y: self.position.y - pixels * 0.50,
            font_pixels: pixels,
        }
    }
}

struct ParticleSystem {
    particles: Vec<Particle>,
    rng: DemoRng,
}

impl ParticleSystem {
    fn new() -> Self {
        let mut system = Self {
            particles: Vec::with_capacity(PARTICLE_COUNT),
            rng: DemoRng::new(0xC0FF_EE51_D310_00A5),
        };
        for _ in 0..PARTICLE_COUNT {
            let mut particle = system.spawn_particle();
            let warmup = system.rng.range(0.0, particle.lifetime * 0.92);
            advance_particle(&mut particle, warmup);
            system.particles.push(particle);
        }
        system
    }

    fn update(&mut self, dt: f32) {
        for index in 0..self.particles.len() {
            let expired = {
                let particle = &mut self.particles[index];
                advance_particle(particle, dt);
                particle.age >= particle.lifetime
                    || particle.position.x < -64.0
                    || particle.position.x > FRAME_WIDTH as f32 + 64.0
                    || particle.position.y > FRAME_HEIGHT as f32 + 64.0
            };
            if expired {
                self.particles[index] = self.spawn_particle();
            }
        }
    }

    fn particles(&self) -> &[Particle] {
        self.particles.as_slice()
    }

    fn spawn_particle(&mut self) -> Particle {
        Particle {
            position: Vec2 {
                x: FRAME_WIDTH as f32 * 0.5 + self.rng.range(-26.0, 26.0),
                y: FRAME_HEIGHT as f32 * 0.80 + self.rng.range(-5.0, 5.0),
            },
            velocity: Vec2 {
                x: self.rng.range(-96.0, 96.0),
                y: self.rng.range(-206.0, -108.0),
            },
            age: 0.0,
            lifetime: self.rng.range(1.6, 3.8),
            size: self.rng.range(22.0, 52.0),
            color: self.rng.next_u32() as usize % COLORS.len(),
        }
    }
}

fn advance_particle(particle: &mut Particle, dt: f32) {
    particle.position.x += particle.velocity.x * dt;
    particle.position.y += particle.velocity.y * dt + 0.5 * GRAVITY * dt * dt;
    particle.velocity.x *= 1.0 - 0.08 * dt;
    particle.velocity.y += GRAVITY * dt;
    particle.age += dt;
}

struct DemoRng(u64);

impl DemoRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

struct ParticleRenderer {
    batches: [Vec<SceneTextRow<'static>>; COLORS.len()],
}

impl ParticleRenderer {
    fn new() -> Self {
        let mut batches = array::from_fn(|_| Vec::with_capacity(PARTICLE_COUNT + HUD_ROW_COUNT));
        batches[0].push(SceneTextRow {
            text: "UI4 GLYPH PARTICLES",
            x: 20.0,
            y: 20.0,
            font_pixels: 20.0,
        });
        batches[0].push(SceneTextRow {
            text: "kernel font scene · no sprite asset",
            x: 20.0,
            y: 47.0,
            font_pixels: 13.0,
        });
        Self { batches }
    }

    fn present(&mut self, frame: &mut Frame, system: &ParticleSystem) -> Result<(), Error> {
        self.batches[0].truncate(HUD_ROW_COUNT);
        for rows in &mut self.batches[1..] {
            rows.clear();
        }
        for particle in system.particles().iter().copied() {
            self.batches[particle.color].push(particle.scene_row());
        }

        frame.begin(BACKGROUND)?;
        for (color, rows) in COLORS.into_iter().zip(self.batches.iter()) {
            if !rows.is_empty() {
                frame.draw_text_scene(
                    Font::Default,
                    (FRAME_WIDTH, FRAME_HEIGHT),
                    color,
                    rows.as_slice(),
                )?;
            }
        }

        frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT))
    }
}

fn main() {
    logl::log(logl::level::INFO, "particle: opening UI4 kernel-font scene");
    let Ok(mut frame) = Frame::open(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(logl::level::ERROR, "particle: UI4 frame open failed");
        return;
    };

    let mut system = ParticleSystem::new();
    let mut renderer = ParticleRenderer::new();
    if let Err(error) = renderer.present(&mut frame, &system) {
        logl::log(
            logl::level::ERROR,
            format_args!("particle: initial UI4 publish failed: {error:?}"),
        );
        return;
    }

    loop {
        vsys::poll_once();
        vsys::sleep_ms(FRAME_MS);
        system.update(FRAME_MS as f32 / 1_000.0);
        if let Err(error) = renderer.present(&mut frame, &system) {
            logl::log(
                logl::level::ERROR,
                format_args!("particle: UI4 publish failed: {error:?}"),
            );
            return;
        }
    }
}
