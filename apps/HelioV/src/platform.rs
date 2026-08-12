//! Narrow VMX vGPU availability probe.
//!
//! This is platform plumbing, not a renderer. It exercises only the generic
//! device/buffer/queue/timeline ABI that a WGPU custom backend can build on.

use trueos::vgpu::{
    BUFFER_USAGE_COPY_DST, BUFFER_USAGE_COPY_SRC, BUFFER_USAGE_MAP_READ, BUFFER_USAGE_MAP_WRITE,
    BUFFER_USAGE_STORAGE, Capabilities, Device, QueueClass,
};

const WITNESS: &[u8] = b"HelioV/VMX/vGPU";

pub struct VgpuProbe {
    pub capabilities: u64,
    pub epoch: u64,
    pub memory_used: u64,
    pub memory_quota: u64,
    pub timeline: u64,
    pub roundtrip_bytes: usize,
}

pub struct ProbeFailure {
    pub stage: &'static str,
    pub code: i32,
}

pub fn probe_vgpu() -> Result<VgpuProbe, ProbeFailure> {
    let device = Device::open(Capabilities::DEFAULT).map_err(|code| fail("open", code))?;
    let result = probe_open_device(device);
    let close_result = device.close();
    match (result, close_result) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(_), Err(code)) => Err(fail("close", code)),
        (Err(failure), _) => Err(failure),
    }
}

fn probe_open_device(device: Device) -> Result<VgpuProbe, ProbeFailure> {
    let info = device.info().map_err(|code| fail("device-info", code))?;
    let buffer = device
        .create_buffer(
            WITNESS.len(),
            BUFFER_USAGE_STORAGE
                | BUFFER_USAGE_COPY_SRC
                | BUFFER_USAGE_COPY_DST
                | BUFFER_USAGE_MAP_READ
                | BUFFER_USAGE_MAP_WRITE,
        )
        .map_err(|code| fail("buffer-create", code))?;

    let buffer_result = (|| {
        let written = device
            .write_buffer(buffer, 0, WITNESS)
            .map_err(|code| fail("buffer-write", code))?;
        let mut readback = [0u8; WITNESS.len()];
        let read = device
            .read_buffer(buffer, 0, &mut readback)
            .map_err(|code| fail("buffer-read", code))?;
        if written != WITNESS.len() || read != WITNESS.len() || readback != WITNESS {
            return Err(fail("buffer-roundtrip", trueos::vgpu::ERR_IO));
        }

        let queue = device
            .create_queue(QueueClass::Render)
            .map_err(|code| fail("queue-create", code))?;
        let queue_result = (|| {
            let point = device
                .submit_control_nop(queue)
                .map_err(|code| fail("queue-submit", code))?;
            device
                .wait(queue, point.value)
                .map_err(|code| fail("queue-wait", code))?;
            Ok((point.value, read))
        })();
        let destroy = device.destroy_queue(queue);
        match (queue_result, destroy) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(code)) => Err(fail("queue-destroy", code)),
            (Err(failure), _) => Err(failure),
        }
    })();

    let destroy = device.destroy_buffer(buffer);
    let (timeline, roundtrip_bytes) = match (buffer_result, destroy) {
        (Ok(value), Ok(())) => value,
        (Ok(_), Err(code)) => return Err(fail("buffer-destroy", code)),
        (Err(failure), _) => return Err(failure),
    };

    Ok(VgpuProbe {
        capabilities: info.capabilities,
        epoch: info.epoch,
        memory_used: info.memory_used,
        memory_quota: info.memory_quota,
        timeline,
        roundtrip_bytes,
    })
}

const fn fail(stage: &'static str, code: i32) -> ProbeFailure {
    ProbeFailure { stage, code }
}
