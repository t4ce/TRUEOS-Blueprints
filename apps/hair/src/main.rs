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
    pub(crate) flow: bool,
    pub(crate) reach: bool,
    pub(crate) body_obstacles: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Controls {
    pub(crate) layers: Layers,
    pub(crate) yaw: f32,
    pub(crate) paused: bool,
    pub(crate) speed_color: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            layers: Layers {
                avatar: true,
                hair: true,
                sphere: true,
                flow: true,
                reach: true,
                body_obstacles: true,
            },
            yaw: 132.0_f32.to_radians(),
            paused: false,
            speed_color: false,
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
            "--speed" => controls.speed_color = true,
            "--hide" => match args.next().as_deref() {
                Some("avatar") => controls.layers.avatar = false,
                Some("hair") => controls.layers.hair = false,
                Some("sphere") => controls.layers.sphere = false,
                Some("flow") => controls.layers.flow = false,
                Some("reach") => controls.layers.reach = false,
                Some("body-obstacles") => controls.layers.body_obstacles = false,
                _ => bail!(
                    "expected a layer name after --hide: avatar, hair, sphere, flow, reach, or body-obstacles"
                ),
            },
            "--help" | "-h" => {
                println!(
                    "hair [--snapshot PATH] [--frames COUNT] [--yaw DEGREES] [--speed] [--hide LAYER]\n\nLayers for --hide: avatar, hair, sphere, flow, reach, body-obstacles\n\n1 Avatar  2 Back hair  3 Head sphere  4 Flow lines  5 Speed/change  6 Hair reach  7 Body obstacles\nLeft/Right Orbit  Space Pause  Esc Close"
                );
                return Ok(());
            }
            _ => bail!("unknown option {arg}; use --help"),
        }
    }

    if let Some(path) = snapshot {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT)?;
        let started = std::time::Instant::now();
        let count = frames.unwrap_or(1);
        for _ in 0..count {
            scene.advance();
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
        if controls.layers.flow {
            println!(
                "hair: {} streamline seeds, {segments} traced segments",
                solver::STREAMLINE_SEEDS
            );
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let mut scene = scene::Scene::new(WIDTH, HEIGHT)?;
        host::run(WIDTH, HEIGHT, controls, frames, |controls, pixels| {
            if !controls.paused {
                scene.advance();
            }
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
