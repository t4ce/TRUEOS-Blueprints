use pulsar_scenedb::{Aabb, SpatialCell};
use trueos::vgpu::{
    BUFFER_USAGE_MAP_READ, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_STORAGE, Device, Queue, VVideoMem,
};

use crate::{dispatch, flush_row_word};

const PAGE_BYTES: usize = 4096;
const STRESS_ROWS: usize = 1024;
const DISPATCHES: usize = 128;
const CHURN_CYCLES: usize = 64;
const RESERVE_CHUNK_BYTES: usize = 2 * 1024 * 1024;
const TARGET_QUOTA_PERCENT: u64 = 94;

struct GpuReport {
    dispatches: usize,
    first_serial: u64,
    last_serial: u64,
}

pub(crate) struct StressReport {
    pub(crate) dispatches: usize,
    pub(crate) churn_cycles: usize,
    pub(crate) quota_rejections: usize,
    pub(crate) peak_bytes: u64,
    pub(crate) quota_bytes: u64,
    pub(crate) peak_buffers: u32,
    pub(crate) first_serial: u64,
    pub(crate) last_serial: u64,
    pub(crate) copied_upload_bytes: u64,
    pub(crate) flushed_vvideo_bytes: u64,
    pub(crate) mapping_digest: u64,
}

pub(crate) fn run(device: Device, queue: Queue) -> Result<StressReport, String> {
    let allocator = pulsar_scenedb::trueos_vvideo::TrueosVVideoAllocator::new(device);
    let mut cell = SpatialCell::new_in(STRESS_ROWS as u32, &allocator)
        .map_err(|error| format!("allocate stress SceneDB page: {error:?}"))?;
    for row in 0..STRESS_ROWS {
        cell.alloc(stress_bounds(row))
            .ok_or_else(|| format!("stress cell became full at row {row}"))?;
    }

    let mut output = device
        .allocate_vvideo_mem(
            STRESS_ROWS * std::mem::size_of::<u32>(),
            BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
        )
        .map_err(|error| format!("allocate stress output vVideoMem: {error}"))?;
    output
        .as_slice_mut::<u32>()
        .map_err(|error| format!("type stress output: {error}"))?
        .fill(u32::MAX);
    output
        .flush(0, output.len())
        .map_err(|error| format!("flush stress output: {error}"))?;

    let mut reserve = fill_to_quota_target(device)?;
    let before = device
        .info()
        .map_err(|error| format!("read pre-stress vGPU info: {error}"))?;
    let over_quota_bytes = usize::try_from(
        before
            .memory_quota
            .saturating_sub(before.memory_used)
            .saturating_add(PAGE_BYTES as u64),
    )
    .map_err(|_| "remaining vGPU quota does not fit usize".to_string())?;
    let quota_rejections = match device.allocate_vvideo_mem(
        over_quota_bytes,
        BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
    ) {
        Err(trueos::vgpu::ERR_OUT_OF_MEMORY) => 1,
        Err(error) => {
            return Err(format!(
                "over-quota allocation returned {error}, expected {}",
                trueos::vgpu::ERR_OUT_OF_MEMORY
            ));
        }
        Ok(unexpected) => {
            drop(unexpected);
            return Err("over-quota vVideoMem allocation unexpectedly succeeded".to_string());
        }
    };

    // TRUEOS implements this Blueprint thread with pthread_create -> the
    // VM-owned background AP service lane. The GPU worker and the Hull-side
    // allocator churn therefore hit the same tenant broker concurrently.
    let gpu_worker = std::thread::spawn(move || run_gpu_worker(device, queue, cell, output));
    let churn_result = churn_reserve(device, &mut reserve);
    let gpu_result = gpu_worker
        .join()
        .map_err(|_| "background SceneDB GPU worker panicked".to_string())?;
    // Even a churn failure joins the worker first, so no guest-owned mapping
    // can outlive the Blueprint's error path.
    churn_result?;
    let (gpu, cell, output) = gpu_result?;

    let peak = device
        .info()
        .map_err(|error| format!("read peak vGPU info: {error}"))?;
    let diagnostics = device
        .diagnostics()
        .map_err(|error| format!("read peak vVideoMem diagnostics: {error}"))?;
    if peak.memory_used.saturating_mul(100) < peak.memory_quota.saturating_mul(TARGET_QUOTA_PERCENT)
    {
        return Err(format!(
            "stress did not reach quota target used={} quota={} target={}pct",
            peak.memory_used, peak.memory_quota, TARGET_QUOTA_PERCENT
        ));
    }
    if diagnostics.copied_upload_bytes != 0 || !diagnostics.mapping_identity() {
        return Err(format!(
            "stress diagnostics invalid copied={} identity={}",
            diagnostics.copied_upload_bytes,
            diagnostics.mapping_identity() as u8
        ));
    }

    let report = StressReport {
        dispatches: gpu.dispatches,
        churn_cycles: CHURN_CYCLES,
        quota_rejections,
        peak_bytes: peak.memory_used,
        quota_bytes: peak.memory_quota,
        peak_buffers: peak.buffer_count,
        first_serial: gpu.first_serial,
        last_serial: gpu.last_serial,
        copied_upload_bytes: diagnostics.copied_upload_bytes,
        flushed_vvideo_bytes: diagnostics.flushed_vvideo_bytes,
        mapping_digest: diagnostics.mapping_digest,
    };

    drop(reserve);
    drop(output);
    drop(cell);
    let retired = device
        .info()
        .map_err(|error| format!("read post-stress vGPU info: {error}"))?;
    if retired.memory_used != 0 || retired.buffer_count != 0 {
        return Err(format!(
            "stress allocations did not retire memory={} buffers={}",
            retired.memory_used, retired.buffer_count
        ));
    }
    Ok(report)
}

fn fill_to_quota_target(device: Device) -> Result<Vec<VVideoMem>, String> {
    let initial = device
        .info()
        .map_err(|error| format!("read vGPU quota: {error}"))?;
    let target = initial.memory_quota.saturating_mul(TARGET_QUOTA_PERCENT) / 100;
    let mut reserve = Vec::new();
    let mut used = initial.memory_used;
    while used < target {
        let remaining = usize::try_from(target - used)
            .map_err(|_| "vGPU quota does not fit usize".to_string())?;
        let bytes = remaining.min(RESERVE_CHUNK_BYTES);
        let memory = device
            .allocate_vvideo_mem(
                bytes,
                BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
            )
            .map_err(|error| format!("fill vVideoMem quota at used={used}: {error}"))?;
        used = used.saturating_add(memory.mapped_len() as u64);
        reserve.push(memory);
    }
    if reserve.is_empty() {
        return Err("vVideoMem quota reserve is empty".to_string());
    }
    Ok(reserve)
}

fn churn_reserve(device: Device, reserve: &mut [VVideoMem]) -> Result<(), String> {
    for cycle in 0..CHURN_CYCLES {
        let index = cycle.wrapping_mul(17) % reserve.len();
        let bytes = reserve[index].len();
        // A one-page placeholder leaves the Vec initialized while the old
        // GPUVM mapping is removed before its equally-sized replacement is
        // allocated. This exercises PPGTT unmap/map and guest-heap reuse.
        let placeholder = device
            .allocate_vvideo_mem(
                PAGE_BYTES,
                BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
            )
            .map_err(|error| format!("allocate churn placeholder cycle={cycle}: {error}"))?;
        let old = std::mem::replace(&mut reserve[index], placeholder);
        drop(old);
        let mut memory = device
            .allocate_vvideo_mem(
                bytes,
                BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
            )
            .map_err(|error| format!("remap churn buffer cycle={cycle} bytes={bytes}: {error}"))?;
        touch_edges(&mut memory, cycle as u8)?;
        let placeholder = std::mem::replace(&mut reserve[index], memory);
        drop(placeholder);
    }
    Ok(())
}

fn touch_edges(memory: &mut VVideoMem, pattern: u8) -> Result<(), String> {
    let edge = memory.len().min(64);
    {
        let bytes = memory.as_bytes_mut();
        bytes[..edge].fill(pattern);
        let tail = bytes.len() - edge;
        bytes[tail..].fill(!pattern);
    }
    memory
        .flush(0, edge)
        .map_err(|error| format!("flush churn head: {error}"))?;
    memory
        .flush(memory.len() - edge, edge)
        .map_err(|error| format!("flush churn tail: {error}"))?;
    memory
        .invalidate(0, edge)
        .map_err(|error| format!("invalidate churn head: {error}"))
}

fn run_gpu_worker(
    device: Device,
    queue: Queue,
    mut cell: SpatialCell,
    mut output: VVideoMem,
) -> Result<(GpuReport, SpatialCell, VVideoMem), String> {
    // Exercise create/flush/destroy from the background AP itself. Without
    // the VM-principal routing in the kernel C ABI, this operation resolves
    // as HostRuntime and fails against the guest-owned device.
    let mut lane_probe = device
        .allocate_vvideo_mem(
            PAGE_BYTES,
            BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
        )
        .map_err(|error| format!("background lane vVideoMem allocation: {error}"))?;
    touch_edges(&mut lane_probe, 0xB7)?;
    drop(lane_probe);

    let query = Aabb {
        min: [-128.0, -128.0, -128.0],
        max: [128.0, 128.0, 128.0],
    };
    let mut cpu = vec![u32::MAX; STRESS_ROWS];
    let mut first_serial = 0;
    let mut last_serial = 0;

    for iteration in 0..DISPATCHES {
        let row = iteration.wrapping_mul(73) % STRESS_ROWS;
        let visible = iteration & 1 == 0;
        {
            let storage = cell.storage_mut();
            storage.user_column_mut::<f32>(0)[row] = if visible { -1.0 } else { 4096.0 };
            storage.user_column_mut::<f32>(1)[row] = if visible { 1.0 } else { 4097.0 };
        }
        if iteration % 5 == 4 {
            cell.liveness().set_dead(row as u32);
        } else {
            cell.liveness().set_live(row as u32);
        }
        flush_row_word(&cell, STRESS_ROWS, 0, row)?;
        flush_row_word(&cell, STRESS_ROWS, 1, row)?;
        flush_liveness_word(&cell, row / 64)?;

        cpu.fill(u32::MAX);
        let cpu_hits = cell.query_aabb(&query, &mut cpu);
        let gpu = dispatch(queue, &cell, &output, query, STRESS_ROWS, iteration == 0)?;
        output
            .invalidate(0, output.len())
            .map_err(|error| format!("invalidate stress output iteration={iteration}: {error}"))?;
        let actual = output
            .as_slice::<u32>()
            .map_err(|error| format!("read stress output iteration={iteration}: {error}"))?;
        if gpu.hits != cpu_hits || actual != cpu.as_slice() {
            return Err(format!(
                "stress AABB mismatch iteration={iteration} cpu_hits={cpu_hits} gpu_hits={}",
                gpu.hits
            ));
        }
        if gpu.point.physical_serial == 0 || gpu.point.physical_serial <= last_serial {
            return Err(format!(
                "stress serial did not advance iteration={iteration} previous={last_serial} current={}",
                gpu.point.physical_serial
            ));
        }
        if first_serial == 0 {
            first_serial = gpu.point.physical_serial;
        }
        last_serial = gpu.point.physical_serial;
    }

    Ok((
        GpuReport {
            dispatches: DISPATCHES,
            first_serial,
            last_serial,
        },
        cell,
        output,
    ))
}

fn flush_liveness_word(cell: &SpatialCell, word: usize) -> Result<(), String> {
    use pulsar_scenedb::trueos_vvideo::TrueosVVideoBacking;

    let backing = cell
        .liveness()
        .backing::<TrueosVVideoBacking>()
        .ok_or_else(|| "stress liveness is not vVideoMem-backed".to_string())?;
    backing
        .memory()
        .flush(
            word * std::mem::size_of::<u64>(),
            std::mem::size_of::<u64>(),
        )
        .map_err(|error| format!("flush stress liveness word={word}: {error}"))
}

fn stress_bounds(row: usize) -> Aabb {
    let x = ((row.wrapping_mul(37) % 2048) as f32) - 1024.0;
    let y = ((row.wrapping_mul(53) % 384) as f32) - 192.0;
    let z = ((row.wrapping_mul(97) % 384) as f32) - 192.0;
    Aabb {
        min: [x, y, z],
        max: [x + 3.0, y + 2.0, z + 1.0],
    }
}
