//! UI4/input-broker presentation loop for the mediated HelioC cloud graph.
//!
//! The frame is acquired through UI4, routed input is sampled through the
//! input broker, and the cloud submission owns the only path to publication.
//! A native `Unsupported` result is a cold/backoff outcome: the frame is
//! discarded and no CPU or alternate renderer is used.

use helio::FlyCamera;
use trueos::{clock, logl, ui4_scene, vsys};

use crate::ui4_input::Ui4FlyInput;
use crate::{cloud_contract, platform::RetainedCloudResources};

const TARGET_HZ: u32 = 30;
const MAX_DT_SECONDS: f32 = 1.0 / 30.0;
const PRESET_SEED: f32 = 19.37;
const PRESET_FOV_DEGREES: f32 = 61.0;
const DAYLIGHT_CYCLE_SECONDS: f32 = 120.0;
const WIND_INITIAL_ANGLE: f32 = 5.0_f32.to_radians();
const WIND_STRENGTH: f32 = 1.95;
const WIND_SPEED: f32 = core::f32::consts::TAU / 180.0;

pub fn run(mut resources: RetainedCloudResources) {
    let (width, height) = match ui4_scene::output_dimensions() {
        Ok(dimensions) => dimensions,
        Err(error) => {
            logl::log(
                logl::level::ERROR,
                format_args!("HelioC: UI4 output admission failed error={error:?}"),
            );
            return;
        }
    };
    let mut frame = match ui4_scene::Frame::open_visual(64, 64, width, height, TARGET_HZ) {
        Ok(frame) => frame,
        Err(error) => {
            logl::log(
                logl::level::ERROR,
                format_args!(
                    "HelioC: UI4 visual window open failed extent={}x{} error={error:?}",
                    width, height
                ),
            );
            return;
        }
    };
    let mut input = Ui4FlyInput::new();
    let mut camera = cloud_contract::reference_camera();
    let start_ns = clock::monotonic_nanos();
    let mut previous_ns = start_ns;
    let mut frame_index = 0u32;
    let mut attempts = 0u64;
    loop {
        vsys::poll_once();
        if let Err(error) = frame.begin_visual_gpu_frame() {
            logl::log(
                logl::level::ERROR,
                format_args!("HelioC: UI4 frame acquire failed error={error:?}"),
            );
            return;
        }
        let (navigation, input_routes) = match input.sample(&mut frame) {
            Ok(sample) => sample,
            Err(error) => {
                logl::log(
                    logl::level::ERROR,
                    format_args!("HelioC: input-broker route read failed error={error:?}"),
                );
                return;
            }
        };
        let now_ns = clock::monotonic_nanos();
        let dt = (now_ns.saturating_sub(previous_ns) as f64 / 1_000_000_000.0) as f32;
        previous_ns = now_ns;
        let dt = dt.clamp(0.0, MAX_DT_SECONDS);
        camera.update(navigation, dt);
        let elapsed = now_ns.saturating_sub(start_ns) as f32 / 1_000_000_000.0;
        if let Err(failure) = resources.update_params(
            &cloud_contract::SimParams {
                time_step: [dt, elapsed, 0.58, 0.0],
                wind_turbulence: {
                    let angle = WIND_INITIAL_ANGLE + elapsed * WIND_SPEED;
                    [
                        angle.sin() * WIND_STRENGTH,
                        0.0,
                        angle.cos() * WIND_STRENGTH,
                        0.84,
                    ]
                },
                brush_center_radius: [0.5, 0.45, 0.64, 0.125],
                brush_controls: [0.38, 0.0, 1.0, 2.0],
                flow_controls: [0.15, 1.41, -0.44, if frame_index == 0 { 1.0 } else { 0.0 }],
                volume_seed: [96.0, 48.0, 96.0, PRESET_SEED],
                art_controls: [1.0, 1.10, 0.72, 0.78],
            },
            &render_params(camera, width, height, elapsed, frame_index),
        ) {
            logl::log(
                logl::level::ERROR,
                format_args!(
                    "HelioC: per-frame parameter update failed stage={} rc={}",
                    failure.stage, failure.code
                ),
            );
            return;
        }
        let surface = match resources.device.acquire_ui4_surface(frame.window_id()) {
            Ok(surface) => surface,
            Err(error) => {
                logl::log(
                    logl::level::ERROR,
                    format_args!("HelioC: UI4 producer surface acquire failed rc={error}"),
                );
                return;
            }
        };
        attempts = attempts.saturating_add(1);
        match resources
            .device
            .submit_cloud_frame(resources.queue, surface, resources.graph, 1)
        {
            Ok(telemetry) => {
                if let Err(error) = frame.publish(ui4_scene::Damage::full(width, height)) {
                    logl::log(
                        logl::level::ERROR,
                        format_args!(
                            "HelioC: cloud submit retired but FULL publish failed rc={error:?}"
                        ),
                    );
                    return;
                }
                logl::log(
                    logl::level::INFO,
                    format_args!(
                        "HelioC: cloud frame published attempts={} routes={} timeline={} steps={} simd={} gpu_active_ns={} damage=FULL",
                        attempts,
                        input_routes,
                        telemetry.point.value,
                        telemetry.simulation_steps,
                        telemetry.simd_width,
                        telemetry.gpu_active_ns,
                    ),
                );
                frame_index = frame_index.wrapping_add(1);
            }
            Err(trueos::vgpu::ERR_UNSUPPORTED) => {
                if attempts == 1 || attempts.is_power_of_two() {
                    logl::log(
                        logl::level::WARN,
                        format_args!(
                            "HelioC: cloud native path cold attempts={} routes={} reason=sealed-native-package-or-direct-presentation-unavailable backoff=UI4-visual-cadence fallback=none",
                            attempts, input_routes
                        ),
                    );
                }
            }
            Err(error) => {
                logl::log(
                    logl::level::ERROR,
                    format_args!(
                        "HelioC: cloud frame submit failed rc={} attempts={} routes={}",
                        error, attempts, input_routes
                    ),
                );
                return;
            }
        }
    }
}

fn render_params(
    camera: FlyCamera,
    width: u32,
    height: u32,
    elapsed: f32,
    frame_index: u32,
) -> cloud_contract::RenderParams {
    let basis = camera.basis();
    let cycle = (elapsed / DAYLIGHT_CYCLE_SECONDS).fract();
    let daylight = if cycle <= 0.5 {
        cycle * 2.0
    } else {
        (1.0 - cycle) * 2.0
    };
    let elevation = (-6.0 + 14.0 * daylight).to_radians();
    let azimuth = 18.0_f32.to_radians();
    let sun = glam::Vec3::new(
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
        azimuth.cos() * elevation.cos(),
    )
    .normalize();
    let exposure = 1.20 + 0.28 * daylight;
    cloud_contract::RenderParams {
        resolution_time: [width as f32, height as f32, elapsed, frame_index as f32],
        camera_position_tan_fov: [
            camera.position().x,
            camera.position().y,
            camera.position().z,
            (PRESET_FOV_DEGREES.to_radians() * 0.5).tan(),
        ],
        camera_forward_exposure: [basis.forward.x, basis.forward.y, basis.forward.z, exposure],
        camera_right_steps: [basis.right.x, basis.right.y, basis.right.z, 70.0],
        camera_up_detail: [
            basis.right.cross(basis.forward).x,
            basis.right.cross(basis.forward).y,
            basis.right.cross(basis.forward).z,
            0.96,
        ],
        sun_direction_intensity: [sun.x, sun.y, sun.z, 1.31],
        sun_color_extinction: [1.0, 0.871, 0.651, 1.45],
        sky_top_ambient: [0.07, 0.16, 0.42, 0.27],
        sky_horizon_seed: [0.74, 0.57, 0.43, PRESET_SEED],
        bounds_min_density: [-16.0, 0.75, -10.5, 1.32],
        bounds_max_shadow: [16.0, 6.25, 27.5, 1.48],
        options: [0.58, 0.0, 0.0, 0.0],
        art_style: [1.0, 5.0, 0.62, 0.78],
        art_cloud_color: [0.038, 0.297, 0.287, 0.20],
        art_shadow_color: [0.002, 0.093, 0.086, 0.72],
        art_sky_color: [0.002, 0.05, 0.054, 0.18],
        art_moon_color: [1.0, 0.807, 0.175, 1.15],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_uses_authored_imported_camera_and_raymarch_defaults() {
        let camera = cloud_contract::reference_camera();
        assert!((camera.yaw() - core::f32::consts::PI).abs() < 1.0e-6);
        let params = render_params(camera, 1600, 900, 0.0, 0);
        let basis = camera.basis();
        let up = basis.right.cross(basis.forward);
        assert_eq!(params.camera_right_steps[3], 70.0);
        assert_eq!(params.options, [0.58, 0.0, 0.0, 0.0]);
        assert!((params.camera_up_detail[0] - up.x).abs() < 1.0e-6);
        assert!((params.camera_up_detail[1] - up.y).abs() < 1.0e-6);
        assert!((params.camera_up_detail[2] - up.z).abs() < 1.0e-6);
    }
}
