//! VMX residency rung for HelioC.
//!
//! This owns no rendering policy. It proves that the Blueprint can retain the
//! exact cloud allocations and parameter uploads in its isolated GPUVM while
//! the authenticated image/sampler and pass-schedule lowering remains cold.

use trueos::vgpu::{
    self, BUFFER_USAGE_COPY_DST, BUFFER_USAGE_COPY_SRC, BUFFER_USAGE_STORAGE, Capabilities, Device,
    VVideoMem,
};

use crate::cloud_contract::{RenderParams, SimParams, VOLUME_BYTES};

const VOLUME_USAGE: u32 = BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_SRC | BUFFER_USAGE_COPY_DST;
const PARAM_USAGE: u32 = BUFFER_USAGE_STORAGE | BUFFER_USAGE_COPY_DST;

pub struct RetainedCloudResources {
    pub volume_a: VVideoMem,
    pub volume_b: VVideoMem,
    pub sim_params: VVideoMem,
    pub render_params: VVideoMem,
}

pub struct ResidencyReport {
    pub capabilities: u64,
    pub volume_bytes: usize,
    pub pair_bytes: usize,
    pub sim_param_bytes: usize,
    pub render_param_bytes: usize,
    pub mapped_bytes: usize,
}

pub struct Failure {
    pub stage: &'static str,
    pub code: i32,
}

impl RetainedCloudResources {
    pub fn allocate() -> Result<(Self, ResidencyReport), Failure> {
        // DEFAULT includes the broker-mandatory BUFFER, QUEUE, and TIMELINE
        // capabilities as well as the compute/render paths used by HelioC.
        let required = Capabilities::DEFAULT.union(Capabilities::PRESENT);
        let device = Device::open(required).map_err(|code| fail("device-open", code))?;
        let result = Self::allocate_on(device);
        if result.is_err() {
            let _ = device.close();
        }
        result
    }

    fn allocate_on(device: Device) -> Result<(Self, ResidencyReport), Failure> {
        let info = device.info().map_err(|code| fail("device-info", code))?;
        let volume_a = device
            .allocate_vvideo_mem(VOLUME_BYTES, VOLUME_USAGE)
            .map_err(|code| fail("volume-a", code))?;
        let volume_b = device
            .allocate_vvideo_mem(VOLUME_BYTES, VOLUME_USAGE)
            .map_err(|code| fail("volume-b", code))?;
        let sim_params = device
            .allocate_vvideo_mem(core::mem::size_of::<SimParams>(), PARAM_USAGE)
            .map_err(|code| fail("sim-params", code))?;
        let render_params = device
            .allocate_vvideo_mem(core::mem::size_of::<RenderParams>(), PARAM_USAGE)
            .map_err(|code| fail("render-params", code))?;

        for (stage, allocation, expected) in [
            ("volume-a-info", &volume_a, VOLUME_BYTES),
            ("volume-b-info", &volume_b, VOLUME_BYTES),
            (
                "sim-params-info",
                &sim_params,
                core::mem::size_of::<SimParams>(),
            ),
            (
                "render-params-info",
                &render_params,
                core::mem::size_of::<RenderParams>(),
            ),
        ] {
            let allocation_info = device
                .buffer_info(allocation.buffer())
                .map_err(|code| fail(stage, code))?;
            // The handle reports the broker's page-rounded mapping extent;
            // the VVideoMem wrapper retains the logical byte count requested
            // by the workload separately.  Validate each against its proper
            // source so small parameter buffers (112/272 bytes) are admitted
            // without weakening the mapping-size check.
            if allocation.len() != expected
                || allocation_info.bytes != allocation.mapped_len() as u64
                || !allocation_info.is_vvideo_mem()
            {
                return Err(fail(stage, vgpu::ERR_IO));
            }
        }

        let mapped_bytes = volume_a
            .mapped_len()
            .checked_add(volume_b.mapped_len())
            .and_then(|bytes| bytes.checked_add(sim_params.mapped_len()))
            .and_then(|bytes| bytes.checked_add(render_params.mapped_len()))
            .ok_or_else(|| fail("mapped-byte-count", vgpu::ERR_OUT_OF_MEMORY))?;
        let report = ResidencyReport {
            capabilities: info.capabilities,
            volume_bytes: VOLUME_BYTES,
            pair_bytes: VOLUME_BYTES * 2,
            sim_param_bytes: core::mem::size_of::<SimParams>(),
            render_param_bytes: core::mem::size_of::<RenderParams>(),
            mapped_bytes,
        };
        Ok((
            Self {
                volume_a,
                volume_b,
                sim_params,
                render_params,
            },
            report,
        ))
    }

    pub fn upload_params(&mut self, sim: &SimParams, render: &RenderParams) -> Result<(), Failure> {
        let sim_bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(sim).cast::<u8>(),
                core::mem::size_of::<SimParams>(),
            )
        };
        let render_bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(render).cast::<u8>(),
                core::mem::size_of::<RenderParams>(),
            )
        };
        self.sim_params.as_bytes_mut().copy_from_slice(sim_bytes);
        self.render_params
            .as_bytes_mut()
            .copy_from_slice(render_bytes);
        self.sim_params
            .flush(0, sim_bytes.len())
            .map_err(|code| fail("sim-params-flush", code))?;
        self.render_params
            .flush(0, render_bytes.len())
            .map_err(|code| fail("render-params-flush", code))?;
        Ok(())
    }
}

const fn fail(stage: &'static str, code: i32) -> Failure {
    Failure { stage, code }
}
