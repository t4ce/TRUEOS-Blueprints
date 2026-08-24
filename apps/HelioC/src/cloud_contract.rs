//! Exact, pointer-free application-side contract for HelioC's first workload.
//!
//! The hashes name the hosted Helio reference shaders. The eventual HELIOA
//! package must reproduce them exactly; TRUEOS never parses or compiles WGSL at
//! runtime.

use helio::{FlyCamera, FlyCameraConfig};

pub const SIMULATION_WGSL_SOURCE: &str =
    "Helio-Examples/cloud-engine-webgpu-linux-aligned/shaders/simulate.wgsl";
pub const RENDER_WGSL_SOURCE: &str =
    "Helio-Examples/cloud-engine-webgpu-linux-aligned/shaders/render.wgsl";
pub const SIMULATION_WGSL_SHA256: &str =
    "f583d3c63e5f387a5926281df29b7688eb09eaa5f06119d74fffa70d592013f6";
pub const RENDER_WGSL_SHA256: &str =
    "5d536a468fcb698c3dca79faac0e5a4924fdc8ce2c9ce3a9b5d24c40a84cc9ff";

pub const VOLUME_WIDTH: u32 = 96;
pub const VOLUME_HEIGHT: u32 = 48;
pub const VOLUME_DEPTH: u32 = 96;
pub const BYTES_PER_VOXEL: usize = 8;
pub const ROW_PITCH_BYTES: usize = VOLUME_WIDTH as usize * BYTES_PER_VOXEL;
pub const SLICE_PITCH_BYTES: usize = ROW_PITCH_BYTES * VOLUME_HEIGHT as usize;
pub const VOLUME_BYTES: usize = SLICE_PITCH_BYTES * VOLUME_DEPTH as usize;
pub const VOLUME_PAIR_BYTES: usize = VOLUME_BYTES * 2;

pub const SIMULATION_LOCAL_SIZE: [u32; 3] = [4, 4, 4];
pub const SIMULATION_DISPATCH: [u32; 3] = [
    VOLUME_WIDTH / SIMULATION_LOCAL_SIZE[0],
    VOLUME_HEIGHT / SIMULATION_LOCAL_SIZE[1],
    VOLUME_DEPTH / SIMULATION_LOCAL_SIZE[2],
];

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct SimParams {
    pub time_step: [f32; 4],
    pub wind_turbulence: [f32; 4],
    pub brush_center_radius: [f32; 4],
    pub brush_controls: [f32; 4],
    pub flow_controls: [f32; 4],
    pub volume_seed: [f32; 4],
    pub art_controls: [f32; 4],
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderParams {
    pub resolution_time: [f32; 4],
    pub camera_position_tan_fov: [f32; 4],
    pub camera_forward_exposure: [f32; 4],
    pub camera_right_steps: [f32; 4],
    pub camera_up_detail: [f32; 4],
    pub sun_direction_intensity: [f32; 4],
    pub sun_color_extinction: [f32; 4],
    pub sky_top_ambient: [f32; 4],
    pub sky_horizon_seed: [f32; 4],
    pub bounds_min_density: [f32; 4],
    pub bounds_max_shadow: [f32; 4],
    pub options: [f32; 4],
    pub art_style: [f32; 4],
    pub art_cloud_color: [f32; 4],
    pub art_shadow_color: [f32; 4],
    pub art_sky_color: [f32; 4],
    pub art_moon_color: [f32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Compute,
    Fragment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceRole {
    SimParams,
    RenderParams,
    VolumeSampled,
    VolumeStorageWrite,
    VolumeSampler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    pub stage: Stage,
    pub group: u16,
    pub binding: u16,
    pub role: ResourceRole,
}

pub const BINDINGS: [Binding; 7] = [
    Binding {
        stage: Stage::Compute,
        group: 0,
        binding: 0,
        role: ResourceRole::SimParams,
    },
    Binding {
        stage: Stage::Compute,
        group: 0,
        binding: 1,
        role: ResourceRole::VolumeSampled,
    },
    Binding {
        stage: Stage::Compute,
        group: 0,
        binding: 2,
        role: ResourceRole::VolumeSampler,
    },
    Binding {
        stage: Stage::Compute,
        group: 0,
        binding: 3,
        role: ResourceRole::VolumeStorageWrite,
    },
    Binding {
        stage: Stage::Fragment,
        group: 0,
        binding: 0,
        role: ResourceRole::RenderParams,
    },
    Binding {
        stage: Stage::Fragment,
        group: 0,
        binding: 1,
        role: ResourceRole::VolumeSampled,
    },
    Binding {
        stage: Stage::Fragment,
        group: 0,
        binding: 2,
        role: ResourceRole::VolumeSampler,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    UpdateSimParams,
    UpdateRenderParams,
    DispatchSimulation,
    ComputeToFragmentVisibility,
    BeginUi4ColorPass,
    DrawFullscreenTriangle,
    PublishUi4,
}

pub const PASS_SCHEDULE: [Command; 7] = [
    Command::UpdateSimParams,
    Command::UpdateRenderParams,
    Command::DispatchSimulation,
    Command::ComputeToFragmentVisibility,
    Command::BeginUi4ColorPass,
    Command::DrawFullscreenTriangle,
    Command::PublishUi4,
];

pub fn reference_camera() -> FlyCamera {
    FlyCamera::new(
        glam::Vec3::new(-1.1756307, 1.5082186, -6.268016),
        core::f32::consts::PI,
        0.084796175,
        FlyCameraConfig::default(),
    )
}

pub fn validate() -> Result<(), &'static str> {
    if core::mem::size_of::<SimParams>() != 112
        || core::mem::align_of::<SimParams>() != 16
        || core::mem::size_of::<RenderParams>() != 272
        || core::mem::align_of::<RenderParams>() != 16
    {
        return Err("parameter ABI");
    }
    if ROW_PITCH_BYTES != 768
        || SLICE_PITCH_BYTES != 36_864
        || VOLUME_BYTES != 3_538_944
        || VOLUME_PAIR_BYTES != 7_077_888
        || SIMULATION_DISPATCH != [24, 12, 24]
    {
        return Err("volume geometry");
    }
    if SIMULATION_WGSL_SHA256.len() != 64
        || RENDER_WGSL_SHA256.len() != 64
        || SIMULATION_WGSL_SOURCE == RENDER_WGSL_SOURCE
        || BINDINGS.len() != 7
        || PASS_SCHEDULE.len() != 7
    {
        return Err("authored WGSL profile");
    }
    Ok(())
}

pub fn custom_device_interface() -> &'static str {
    core::any::type_name::<dyn wgpu::custom::DeviceInterface>()
}

const _: () = {
    assert!(core::mem::size_of::<SimParams>() == 112);
    assert!(core::mem::align_of::<SimParams>() == 16);
    assert!(core::mem::size_of::<RenderParams>() == 272);
    assert!(core::mem::align_of::<RenderParams>() == 16);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_cloud_profile_is_stable() {
        assert_eq!(validate(), Ok(()));
        assert_eq!(BINDINGS.len(), 7);
        let _ = reference_camera();
    }
}
