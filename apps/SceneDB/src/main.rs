use pulsar_scenedb::{Aabb, NULL_ROW, SpatialCell};

const ROWS: usize = 256;
const OUTPUT_ROWS: usize = ROWS + 4;
const TRAILING_TOKEN: u32 = 0xA5A5_A5A5;

fn bounds(row: usize) -> Aabb {
    let x = row as f32;
    Aabb {
        min: [x, 0.0, 0.0],
        max: [x + 0.5, 1.0, 1.0],
    }
}

fn query() -> Aabb {
    Aabb {
        min: [40.0, 0.0, 0.0],
        max: [60.0, 1.0, 1.0],
    }
}

fn sparse_query() -> Aabb {
    Aabb {
        min: [42.25, 0.5, 0.5],
        max: [42.25, 0.5, 0.5],
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn main() {
    let mut cell = SpatialCell::new(ROWS as u32).expect("SceneDB page layout");
    let mut handles = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        handles.push(cell.alloc(bounds(row)).expect("cell capacity"));
    }

    let mut all_live = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    assert_eq!(cell.query_aabb(&query(), &mut all_live), 21);
    assert_eq!(all_live[40], 40);
    assert_eq!(all_live[60], 60); // closed interval: touching face is a hit
    assert!(
        all_live[ROWS..]
            .iter()
            .all(|token| *token == TRAILING_TOKEN)
    );

    cell.storage_mut().user_column_mut::<f32>(0)[50] = f32::NAN;
    assert!(cell.free(handles[44]));
    assert!(cell.free(handles[55]));
    let mut oracle = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    let hits = cell.query_aabb(&query(), &mut oracle);
    assert_eq!(hits, 18);
    assert_eq!(oracle[44], NULL_ROW);
    assert_eq!(oracle[50], NULL_ROW); // ordered NaN comparison is false
    assert_eq!(oracle[55], NULL_ROW);
    assert!(oracle[ROWS..].iter().all(|token| *token == TRAILING_TOKEN));

    let mut sparse = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    assert_eq!(cell.query_aabb(&sparse_query(), &mut sparse), 1);
    assert_eq!(sparse[42], 42);
    assert_eq!(sparse[41], NULL_ROW);
    assert_eq!(sparse[43], NULL_ROW);

    cell.storage_mut().user_column_mut::<f32>(0)[100] = 50.0;
    cell.storage_mut().user_column_mut::<f32>(1)[100] = 50.5;
    let mut mutated = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    assert_eq!(cell.query_aabb(&query(), &mut mutated), hits + 1);
    println!(
        "SceneDB headless CPU oracle passed rows={ROWS} cases=all-live,dead,sparse,touching,NaN,mutation; vVideoMem proof runs on TRUEOS"
    );
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn main() {
    if let Err(error) = run_trueos_proof() {
        panic!("SceneDB vVideoMem proof failed: {error}");
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn run_trueos_proof() -> Result<(), String> {
    use pulsar_scenedb::trueos_vvideo::{TrueosVVideoAllocator, TrueosVVideoBacking};
    use trueos::vgpu::{
        BUFFER_USAGE_MAP_READ, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_STORAGE, Capabilities, Device,
        QueueClass,
    };

    let device =
        Device::open(Capabilities::DEFAULT).map_err(|error| format!("open vGPU: {error}"))?;
    let queue = device
        .create_queue(QueueClass::Compute)
        .map_err(|error| format!("create compute queue: {error}"))?;
    let allocator = TrueosVVideoAllocator::new(device);
    let mut cell = SpatialCell::new_in(ROWS as u32, &allocator)
        .map_err(|error| format!("allocate SceneDB page: {error:?}"))?;
    let mut handles = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        handles.push(
            cell.alloc(bounds(row))
                .ok_or_else(|| "cell full".to_string())?,
        );
    }

    let mut output = device
        .allocate_vvideo_mem(
            OUTPUT_ROWS * std::mem::size_of::<u32>(),
            BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_STORAGE,
        )
        .map_err(|error| format!("allocate output vVideoMem: {error}"))?;
    output
        .as_slice_mut::<u32>()
        .map_err(|error| format!("typed output access: {error}"))?
        .fill(TRAILING_TOKEN);
    output
        .flush(0, output.len())
        .map_err(|error| format!("flush output: {error}"))?;

    let q = query();
    let mut cpu_all_live = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    let cpu_all_live_hits = cell.query_aabb(&q, &mut cpu_all_live);
    let all_live = dispatch(queue, &cell, &output, q, ROWS, true)?;
    output
        .invalidate(0, ROWS * std::mem::size_of::<u32>())
        .map_err(|error| format!("invalidate all-live output: {error}"))?;
    if all_live.hits != cpu_all_live_hits
        || output
            .as_slice::<u32>()
            .map_err(|error| format!("read all-live output: {error}"))?
            != cpu_all_live.as_slice()
        || cpu_all_live_hits != 21
    {
        return Err(format!(
            "all-live/touching dispatch mismatch cpu_hits={} gpu_hits={}",
            cpu_all_live_hits, all_live.hits
        ));
    }

    cell.storage_mut().user_column_mut::<f32>(0)[50] = f32::NAN;
    if !cell.free(handles[44]) || !cell.free(handles[55]) {
        return Err("failed to mark deterministic dead rows".to_string());
    }
    let mut cpu_first = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    let cpu_first_hits = cell.query_aabb(&q, &mut cpu_first);
    let first = dispatch(queue, &cell, &output, q, ROWS, true)?;
    output
        .invalidate(0, ROWS * std::mem::size_of::<u32>())
        .map_err(|error| format!("invalidate first output: {error}"))?;
    let gpu_first = output
        .as_slice::<u32>()
        .map_err(|error| format!("read first output: {error}"))?;
    if first.hits != cpu_first_hits
        || gpu_first != cpu_first.as_slice()
        || cpu_first_hits != 18
        || gpu_first[44] != NULL_ROW
        || gpu_first[50] != NULL_ROW
        || gpu_first[55] != NULL_ROW
    {
        return Err(format!(
            "first dispatch mismatch cpu_hits={} gpu_hits={}",
            cpu_first_hits, first.hits
        ));
    }

    let sparse_query = sparse_query();
    let mut cpu_sparse = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    let cpu_sparse_hits = cell.query_aabb(&sparse_query, &mut cpu_sparse);
    let sparse = dispatch(queue, &cell, &output, sparse_query, ROWS, false)?;
    output
        .invalidate(0, ROWS * std::mem::size_of::<u32>())
        .map_err(|error| format!("invalidate sparse output: {error}"))?;
    if sparse.hits != 1
        || sparse.hits != cpu_sparse_hits
        || output
            .as_slice::<u32>()
            .map_err(|error| format!("read sparse output: {error}"))?
            != cpu_sparse.as_slice()
    {
        return Err(format!(
            "sparse dispatch mismatch cpu_hits={} gpu_hits={}",
            cpu_sparse_hits, sparse.hits
        ));
    }

    let before_empty = output
        .as_slice::<u32>()
        .map_err(|error| format!("read pre-empty output: {error}"))?
        .to_vec();
    let empty = dispatch(queue, &cell, &output, q, 0, false)?;
    if empty.hits != 0
        || empty.point.physical_serial != 0
        || output
            .as_slice::<u32>()
            .map_err(|error| format!("read empty output: {error}"))?
            != before_empty.as_slice()
    {
        return Err("empty dispatch changed output or reached hardware".to_string());
    }

    // Change one previously missed row directly in the same vVideoMem page.
    // Only its two X-bound words are published; no buffer upload occurs.
    cell.storage_mut().user_column_mut::<f32>(0)[100] = 50.0;
    cell.storage_mut().user_column_mut::<f32>(1)[100] = 50.5;
    flush_row_word(&cell, 0, 100)?;
    flush_row_word(&cell, 1, 100)?;

    let mut cpu_second = vec![TRAILING_TOKEN; OUTPUT_ROWS];
    let cpu_second_hits = cell.query_aabb(&q, &mut cpu_second);
    let second = dispatch(queue, &cell, &output, q, ROWS, false)?;
    output
        .invalidate(0, ROWS * std::mem::size_of::<u32>())
        .map_err(|error| format!("invalidate second output: {error}"))?;
    let gpu_second = output
        .as_slice::<u32>()
        .map_err(|error| format!("read second output: {error}"))?;
    if second.hits != cpu_second_hits
        || gpu_second != cpu_second.as_slice()
        || cpu_second_hits != cpu_first_hits + 1
    {
        return Err(format!(
            "mutation dispatch mismatch first={} cpu={} gpu={}",
            cpu_first_hits, cpu_second_hits, second.hits
        ));
    }

    let diagnostics = device
        .diagnostics()
        .map_err(|error| format!("read vVideoMem diagnostics: {error}"))?;
    if diagnostics.copied_upload_bytes != 0
        || diagnostics.flushed_vvideo_bytes == 0
        || !diagnostics.mapping_identity()
        || diagnostics.vvideo_buffers < 3
        || all_live.point.physical_serial == 0
        || first.point.physical_serial == 0
        || first.point.physical_serial <= all_live.point.physical_serial
        || sparse.point.physical_serial <= first.point.physical_serial
        || second.point.physical_serial <= sparse.point.physical_serial
    {
        return Err(format!(
            "proof diagnostics invalid copied={} flushed={} identity={} buffers={} serials={}->{}",
            diagnostics.copied_upload_bytes,
            diagnostics.flushed_vvideo_bytes,
            diagnostics.mapping_identity() as u8,
            diagnostics.vvideo_buffers,
            first.point.physical_serial,
            second.point.physical_serial,
        ));
    }

    println!(
        "SceneDB vVideoMem PASS rows={} cases=all-live,dead,sparse,touching,empty,NaN,mutation first_hits={} second_hits={} serials={}->{} copied_upload_bytes={} flushed_vvideo_bytes={} mapping_identity={} mapping_digest=0x{:016X} trailing_untouched=1",
        ROWS,
        first.hits,
        second.hits,
        first.point.physical_serial,
        second.point.physical_serial,
        diagnostics.copied_upload_bytes,
        diagnostics.flushed_vvideo_bytes,
        diagnostics.mapping_identity() as u8,
        diagnostics.mapping_digest,
    );
    drop(output);
    drop(cell);
    device
        .destroy_queue(queue)
        .map_err(|error| format!("destroy queue: {error}"))?;
    device
        .close()
        .map_err(|error| format!("close vGPU: {error}"))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn flush_row_word(cell: &SpatialCell, column: usize, row: usize) -> Result<(), String> {
    use pulsar_scenedb::trueos_vvideo::TrueosVVideoBacking;
    let storage = cell.storage();
    let range = storage.user_column_byte_range(column, ROWS as u32);
    let backing = storage
        .page_backing::<TrueosVVideoBacking>()
        .ok_or_else(|| "SceneDB page is not vVideoMem-backed".to_string())?;
    let offset = range.start + row * std::mem::size_of::<f32>();
    backing
        .memory()
        .flush(offset, std::mem::size_of::<f32>())
        .map_err(|error| format!("flush column {column} row {row}: {error}"))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn dispatch(
    queue: trueos::vgpu::Queue,
    cell: &SpatialCell,
    output: &trueos::vgpu::VVideoMem,
    query: Aabb,
    rows: usize,
    flush_inputs: bool,
) -> Result<trueos::vgpu::SceneAabbResult, String> {
    use pulsar_scenedb::trueos_vvideo::TrueosVVideoBacking;
    use trueos::vgpu::{BufferSlice, SceneAabbDispatch};

    let storage = cell.storage();
    let page = storage
        .page_backing::<TrueosVVideoBacking>()
        .ok_or_else(|| "SceneDB page is not vVideoMem-backed".to_string())?;
    let mut bounds = [BufferSlice::default(); 6];
    for (column, slice) in bounds.iter_mut().enumerate() {
        if flush_inputs {
            storage
                .flush_user_column(column, rows as u32)
                .map_err(|_| format!("flush bounds column {column}"))?;
        }
        *slice = page
            .slice(storage.user_column_byte_range(column, rows as u32))
            .map_err(|error| format!("slice bounds column {column}: {error}"))?;
    }
    let live_words = rows.div_ceil(64);
    if flush_inputs {
        cell.liveness()
            .flush_words(live_words)
            .map_err(|_| "flush liveness".to_string())?;
    }
    let live = cell
        .liveness()
        .backing::<TrueosVVideoBacking>()
        .ok_or_else(|| "liveness is not vVideoMem-backed".to_string())?
        .slice(0..live_words * std::mem::size_of::<u64>())
        .map_err(|error| format!("slice liveness: {error}"))?;
    let output_slice = output
        .slice(0, output.len())
        .map_err(|error| format!("slice output: {error}"))?;
    queue
        .submit_scene_aabb(&SceneAabbDispatch {
            bounds,
            liveness: live,
            output: output_slice,
            rows: rows as u32,
            reserved: 0,
            query_min: [query.min[0], query.min[1], query.min[2], 0.0],
            query_max: [query.max[0], query.max[1], query.max[2], 0.0],
        })
        .map_err(|error| format!("SceneAabbDispatch: {error}"))
}
