use anyhow::{Context, Result, bail, ensure};
use std::{fs::File, path::PathBuf};

use hair_rope::HairMotion;

#[path = "../shaders/hair_hull.rs"]
mod hair_hull;
mod hair_rod;
mod hair_roots;
mod hair_rope;
mod hair_spring;
#[cfg(target_os = "linux")]
mod host;
mod obstacle;
#[path = "../shaders/raster.rs"]
mod raster;
mod scene;
mod solver;
#[cfg(target_os = "trueos")]
mod ui4;
mod water;
mod water_iso;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 720;
const WATER_PREVIEW: bool = false;
const DRAW_ROD_GUIDES: bool = false;
const TILT_LIMIT: f32 = std::f32::consts::FRAC_PI_2;

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
    pub(crate) view_orbit: f32,
    pub(crate) roll: f32,
    pub(crate) tilt: f32,
    pub(crate) hair_motion: Option<HairMotion>,
    pub(crate) rod_tuning: hair_rope::RodTuning,
    #[cfg(target_os = "trueos")]
    pub(crate) paused: bool,
    pub(crate) speed_color: bool,
    pub(crate) tip_spread: bool,
    pub(crate) water: bool,
    pub(crate) water_volume: f32,
    pub(crate) flow: solver::FlowSettings,
}

impl Default for Controls {
    fn default() -> Self {
        let field_debug = cfg!(target_os = "trueos");
        let hair_motion = cfg!(target_os = "linux").then_some(HairMotion::Rod);
        Self {
            layers: Layers {
                avatar: true,
                hair: true,
                sphere: field_debug,
                guides: default_guides(hair_motion),
                field: field_debug,
                reach: field_debug,
                body_obstacles: true,
            },
            yaw: if field_debug {
                132.0_f32.to_radians()
            } else {
                0.0
            },
            view_orbit: 0.0,
            roll: 0.0,
            tilt: 0.0,
            hair_motion,
            rod_tuning: hair_rope::RodTuning::default(),
            #[cfg(target_os = "trueos")]
            paused: false,
            speed_color: false,
            tip_spread: true,
            water: cfg!(target_os = "linux") && WATER_PREVIEW,
            water_volume: 1.0,
            flow: solver::FlowSettings::DEFAULT,
        }
    }
}

fn default_guides(hair_motion: Option<HairMotion>) -> bool {
    DRAW_ROD_GUIDES || hair_motion != Some(HairMotion::Rod)
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
    let mut guides_overridden = false;
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
            "--tilt" => {
                let degrees = args.next().context("missing tilt angle")?.parse::<f32>()?;
                ensure!(degrees.is_finite(), "tilt angle must be finite");
                controls.tilt = degrees.to_radians().clamp(-TILT_LIMIT, TILT_LIMIT);
            }
            "--motion" => {
                controls.hair_motion = match args.next().as_deref() {
                    Some("1") => Some(HairMotion::Rope),
                    Some("2") => Some(HairMotion::Spring),
                    Some("3") => Some(HairMotion::Rod),
                    Some("flow") => None,
                    _ => bail!("expected 1, 2, 3, or flow after --motion"),
                };
                if !guides_overridden {
                    controls.layers.guides = default_guides(controls.hair_motion);
                }
            }
            "--hull" => {
                ensure!(
                    args.next().as_deref() == Some("locks"),
                    "available hull is locks"
                );
            }
            "--hull-sampling" => {
                ensure!(
                    matches!(args.next().as_deref(), Some("100" | "minimum")),
                    "hull sampling is fixed at 100"
                );
            }
            "--speed" => controls.speed_color = true,
            "--no-tip-spread" => controls.tip_spread = false,
            "--hide" | "--show" => {
                let name = args.next().context("missing layer name")?;
                controls.layers.set_visible(&name, arg == "--show")?;
                guides_overridden |= matches!(name.as_str(), "guides" | "flow");
            }
            "--help" | "-h" => {
                println!(
                    "hair [--snapshot PATH] [--frames COUNT] [--yaw DEGREES] [--roll DEGREES] [--tilt DEGREES] [--motion 1|2|3|flow] [--speed] [--no-tip-spread] [--hide LAYER] [--show LAYER]\n\nLayers: avatar, hair, sphere, guides, field, flow (both line layers), reach, body-obstacles\n\nUbuntu preview: Hold A or D to orbit the view around the avatar. Move the mouse left or right to spin and up or down to tilt. In bending-rod mode, 1 and 2 cycle wind strength and direction. Hold Shift to step backward. Close the window to exit."
                );
                if controls.water {
                    println!("Water preview keys: I/K volume, J/L flow speed, U/O viscosity.");
                }
                return Ok(());
            }
            _ => bail!("unknown option {arg}; use --help"),
        }
    }

    if let Some(path) = snapshot {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT, controls)?;
        let started = std::time::Instant::now();
        let count = frames.unwrap_or(1);
        for _ in 0..count {
            scene.advance(controls);
        }
        let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4];
        let drawn = scene.draw(controls, &mut pixels);
        let mut encoder = png::Encoder::new(File::create(&path)?, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
        println!(
            "hair: rendered {count} solver step(s) to {} in {:.3}s",
            path.display(),
            started.elapsed().as_secs_f64()
        );
        if controls.water {
            println!("hair: medium water surface, {} triangles", drawn.triangles);
        } else if let Some(motion) = controls.hair_motion {
            println!(
                "hair: {}, {} roots, {} strip triangles, {} debug line segments",
                motion.label(),
                scene.guide_count(),
                drawn.triangles,
                drawn.segments,
            );
        } else if controls.layers.field || controls.layers.guides {
            println!(
                "hair: {} field seeds, {} hair-root seeds, {} traced segments",
                if controls.layers.field {
                    solver::FIELD_STREAMLINE_SEEDS
                } else {
                    0
                },
                if controls.layers.guides {
                    scene.guide_count()
                } else {
                    0
                },
                drawn.segments,
            );
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT, controls)?;
        host::run(WIDTH, HEIGHT, controls, frames, |controls, pixels| {
            scene.advance(controls);
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
