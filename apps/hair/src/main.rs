use anyhow::{Context, Result, bail, ensure};
use std::{fs::File, path::PathBuf};

#[cfg(target_os = "linux")]
mod host;
mod obstacle;
#[path = "../shaders/raster.rs"]
mod raster;
mod scene;
mod solver;
#[cfg(target_os = "trueos")]
mod ui4;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 720;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layers {
    pub(crate) avatar: bool,
    pub(crate) hair: bool,
    pub(crate) sphere: bool,
    pub(crate) guides: bool,
    pub(crate) field: bool,
    pub(crate) reach: bool,
    pub(crate) body_obstacles: bool,
}

impl Layers {
    fn set_visible(&mut self, name: &str, visible: bool) -> Result<()> {
        match name {
            "avatar" => self.avatar = visible,
            "hair" => self.hair = visible,
            "sphere" => self.sphere = visible,
            "guides" => self.guides = visible,
            "field" => self.field = visible,
            "flow" => {
                self.guides = visible;
                self.field = visible;
            }
            "reach" => self.reach = visible,
            "body-obstacles" => self.body_obstacles = visible,
            _ => bail!("unknown layer {name}; use --help"),
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Controls {
    pub(crate) layers: Layers,
    pub(crate) yaw: f32,
    pub(crate) roll: f32,
    #[cfg(target_os = "trueos")]
    pub(crate) paused: bool,
    pub(crate) speed_color: bool,
    pub(crate) tip_spread: bool,
}

impl Default for Controls {
    fn default() -> Self {
        let field_debug = cfg!(target_os = "trueos");
        Self {
            layers: Layers {
                avatar: true,
                hair: true,
                sphere: field_debug,
                guides: true,
                field: field_debug,
                reach: field_debug,
                body_obstacles: true,
            },
            yaw: if field_debug {
                132.0_f32.to_radians()
            } else {
                0.0
            },
            roll: 0.0,
            #[cfg(target_os = "trueos")]
            paused: false,
            speed_color: false,
            tip_spread: true,
        }
    }
}

#[cfg(target_os = "trueos")]
impl Controls {
    fn roll_by(&mut self, direction: f32, elapsed: f32) {
        const ROLL_SPEED: f32 = 0.6;
        if !self.paused {
            self.roll =
                (self.roll + direction * ROLL_SPEED * elapsed).rem_euclid(std::f32::consts::TAU);
        }
    }
}

fn main() -> Result<()> {
    let mut controls = Controls::default();
    let mut snapshot = None;
    let mut frames = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--snapshot" => {
                snapshot = Some(PathBuf::from(args.next().context("missing PNG path")?))
            }
            "--frames" => {
                let count = args
                    .next()
                    .context("missing frame count")?
                    .parse::<usize>()?;
                ensure!(count > 0, "frame count must be positive");
                frames = Some(count);
            }
            "--yaw" => {
                let degrees = args.next().context("missing yaw angle")?.parse::<f32>()?;
                ensure!(degrees.is_finite(), "yaw angle must be finite");
                controls.yaw = degrees.rem_euclid(360.0).to_radians();
            }
            "--roll" => {
                let degrees = args.next().context("missing roll angle")?.parse::<f32>()?;
                ensure!(degrees.is_finite(), "roll angle must be finite");
                controls.roll = degrees.rem_euclid(360.0).to_radians();
            }
            "--speed" => controls.speed_color = true,
            "--no-tip-spread" => controls.tip_spread = false,
            "--hide" | "--show" => {
                let name = args.next().context("missing layer name")?;
                controls.layers.set_visible(&name, arg == "--show")?;
            }
            "--help" | "-h" => {
                println!(
                    "hair [--snapshot PATH] [--frames COUNT] [--yaw DEGREES] [--roll DEGREES] [--speed] [--no-tip-spread] [--hide LAYER] [--show LAYER]\n\nLayers: avatar, hair, sphere, guides, field, flow (both line layers), reach, body-obstacles\n\nUbuntu: Move the mouse left or right to spin. Close the window to exit."
                );
                return Ok(());
            }
            _ => bail!("unknown option {arg}; use --help"),
        }
    }

    if let Some(path) = snapshot {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT, controls.roll)?;
        let started = std::time::Instant::now();
        let count = frames.unwrap_or(1);
        for _ in 0..count {
            scene.advance(controls.roll, controls.tip_spread);
        }
        let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4];
        let segments = scene.draw(controls, &mut pixels);
        let mut encoder = png::Encoder::new(File::create(&path)?, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
        println!(
            "hair: rendered {count} solver step(s) to {} in {:.3}s",
            path.display(),
            started.elapsed().as_secs_f64()
        );
        if controls.layers.field || controls.layers.guides {
            println!(
                "hair: {} field seeds, {} hair-root seeds, {segments} traced segments",
                if controls.layers.field {
                    solver::FIELD_STREAMLINE_SEEDS
                } else {
                    0
                },
                if controls.layers.guides {
                    solver::ROOT_STREAMLINE_SEEDS
                } else {
                    0
                },
            );
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT, controls.roll)?;
        host::run(WIDTH, HEIGHT, controls, frames, |controls, pixels| {
            scene.advance(controls.roll, controls.tip_spread);
            scene.draw(controls, pixels);
            Ok(())
        })
    }
    #[cfg(target_os = "trueos")]
    {
        ui4::run(WIDTH, HEIGHT, controls, frames)
    }
    #[cfg(not(any(target_os = "linux", target_os = "trueos")))]
    bail!("use --snapshot on this platform");
}
