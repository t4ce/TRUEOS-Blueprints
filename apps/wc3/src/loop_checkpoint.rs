//! Memoize audited guest-only loops, including every SEH scratch write.
//! No guessed register results and no suppression of guest debug exceptions.
use super::*;
use wc3::checkpoint::{Boundary, TableCheckpoint, TableCheckpointPage};

struct LoopRegion {
    name: &'static str,
    boundary: Boundary,
    code_start: u32,
    code_hash: [u8; 32],
    path: &'static [u8],
}

const REGIONS: [LoopRegion; 2] = [
    LoopRegion {
        name: "decrypt-scan",
        boundary: Boundary {
            from: 0x45af54,
            to: 0x45b005,
            table_bytes: 0,
        },
        code_start: 0x45af54,
        code_hash: HASH_SCAN,
        path: b"/common/Warcraft III/.trueos-wc3/decrypt-scan-v1.bin",
    },
    LoopRegion {
        name: "dword-checksum",
        boundary: Boundary {
            from: 0x45b0b1,
            to: 0x45b0e1,
            table_bytes: 0,
        },
        code_start: 0x45b0a2,
        code_hash: HASH_SUM,
        path: b"/common/Warcraft III/.trueos-wc3/dword-checksum-v1.bin",
    },
];

pub(crate) struct LoopCheckpointCapture {
    region: usize,
    before: TableCheckpoint,
    pages: Vec<CachedPage>,
    steps: u64,
}

fn identity(child: &PendingChild) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"WC3 audited loops v1");
    hash.update(child.self_image_bytes.as_slice());
    // A change in the host-side execution/SEH semantics invalidates these caches.
    hash.update(include_bytes!("main.rs"));
    hash.update(include_bytes!("asupersync.rs"));
    hash.update(include_bytes!("loop_checkpoint.rs"));
    hash.update(include_bytes!("seh.rs"));
    hash.update(include_bytes!("checkpoint.rs"));
    hash.finalize().into()
}

fn hash_matches(child: &PendingChild, start: u32, len: usize, expected: [u8; 32]) -> bool {
    let mut bytes = vec![0; len];
    checkpoint_read(child, start, &mut bytes).is_ok() && checkpoint_sha256(&bytes) == expected
}

fn contains(start: u32, end: u32, address: u32, len: u32) -> bool {
    address >= start && address.checked_add(len).is_some_and(|last| last <= end)
}

fn guards(child: &PendingChild, region: &LoopRegion, registers: Registers) -> bool {
    let heap =
        |address, len| contains(CHILD_WIN_HEAP_BASE, child.win_heap_mapped_end, address, len);
    let image_end = child
        .image
        .image_base
        .checked_add(child.image.image.len() as u32);
    let input = |address, len| {
        heap(address, len)
            || image_end.is_some_and(|end| contains(child.image.image_base, end, address, len))
    };
    let read = |address| child_read_u32(child, address);
    if table_checkpoint_quiescent(child).is_err()
        || child.load_library_call.is_some()
        || child.table_checkpoint_capture.is_some()
        || current_child_seh_handler(child, registers.fs_base) != Some(WAR3_DIVIDE_EXCEPTION_HANDLER)
        || registers.fs_base != thread_teb_va(child.tid).unwrap_or(0)
        || !contains(STACK_BASE, STACK_BASE + STACK_BYTES as u32, registers.esp.saturating_sub(4096), 4100)
        || !hash_matches(child, region.code_start, (region.boundary.to - region.code_start) as usize, region.code_hash)
        || !hash_matches(child, WAR3_DIVIDE_EXCEPTION_HANDLER, 0x3bd, HASH_HANDLER)
        // Keep the handler's instruction-byte reads within this audited loop.
        || !read(0x49aa74).is_some_and(|low| low <= region.code_start)
        || !read(0x49a96c).is_some_and(|high| high >= region.boundary.to)
        || read(0x49a588) != Some(0)
        || read(WAR3_SCAN_GATE) != Some(1)
    {
        return false;
    }
    if region.boundary.from == 0x45af54 {
        let Some(bound) = read(WAR3_SCAN_BOUND).filter(|n| *n <= 0x1000000) else {
            return false;
        };
        read(WAR3_SCAN_INDEX) == Some(0)
            && read(WAR3_SCAN_SOURCE).is_some_and(|n| n <= 64)
            && read(WAR3_DIVIDE_TABLE_BASE_SLOT).is_some_and(|base| heap(base, bound))
            && read(0x49aecc).is_some_and(|base| input(base, 4096))
            // The second XOR destination is fixed data, never executable bytes.
            && read(0x49c764) == Some(0x470990)
            && image_end.is_some_and(|end| contains(child.image.image_base, end, 0x470990, 80))
    } else {
        let Some(bytes) = read(0x49c524).filter(|n| *n <= 0x1000000) else {
            return false;
        };
        read(WAR3_DWORD_SCAN_INDEX) == Some(0)
            && read(0x49ddb8).is_some_and(|base| input(base, bytes & !3))
            && read(0x49b030).is_some_and(|base| heap(base, 4))
    }
}

fn pages(child: &PendingChild) -> Result<Vec<CachedPage>, String> {
    let mut ranges = vec![
        (child.image.image_base, child.image.image.len() as u32),
        (PROCESS_DATA_VA, 4096),
        (thread_teb_va(child.tid)?, 4096),
        (STACK_BASE, STACK_BYTES as u32),
    ];
    for (base, end) in [
        (CHILD_WIN_HEAP_BASE, child.win_heap_mapped_end),
        (CHILD_CRT_HEAP_BASE, child.crt_heap_mapped_end),
    ] {
        let len = end.checked_sub(base).ok_or("loop checkpoint heap range")?;
        if len != 0 {
            ranges.push((base, len));
        }
    }
    checkpoint_capture_pages(child, &ranges)
}

// Call before dispatching every exit. Interleaving another context or any
// provider/API activity invalidates a capture before it can be persisted.
pub(super) fn observe_exit(child: &mut PendingChild, key: ThreadKey, exit: &trueos::x86::Exit) {
    if child.loop_checkpoint_capture.is_none() {
        return;
    }
    let allowed = key.pid == child.pid
        && key.tid == child.tid
        && match exit.kind {
            ExitKind::VmCall => exit.registers.eip == thunk32::CHILD_SEH_RETURN_AFTER_VMCALL,
            ExitKind::Exception => {
                decode_child_exception(exit.detail, exit.qualification).vector == Some(1)
            }
            ExitKind::Other => exit.detail == 52,
            _ => false,
        };
    if !allowed {
        discard(child, "external-execution");
    }
}

fn discard(child: &mut PendingChild, reason: &str) {
    if let Some(capture) = child.loop_checkpoint_capture.take() {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD LOOP CHECKPOINT DISCARD region={} reason={reason}",
                REGIONS[capture.region].name
            ),
        );
    }
}

pub(super) async fn boundary(
    child: &mut PendingChild,
    context: &mut Context,
    registers: &mut Registers,
    debug: &mut DebugRegisters,
) -> Result<(), String> {
    if cfg!(feature = "replay-loops") {
        return Ok(());
    }
    if let Some(capture) = child.loop_checkpoint_capture.as_ref() {
        let region = &REGIONS[capture.region];
        if registers.eip == region.boundary.to {
            let capture = child.loop_checkpoint_capture.take().unwrap();
            let result = finish(child, context, *registers, *debug, capture).await;
            if let Err(error) = result {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD LOOP CHECKPOINT BYPASS region={} reason={error}",
                        region.name
                    ),
                );
            }
            return Ok(());
        }
        if !(region.code_start..region.boundary.to).contains(&registers.eip)
            || child.single_step_count.saturating_sub(capture.steps) > 1_000_000
        {
            discard(child, "left-audited-region");
        }
        return Ok(());
    }
    let Some(index) = REGIONS
        .iter()
        .position(|region| region.boundary.from == registers.eip)
    else {
        return Ok(());
    };
    if child.loop_checkpoint_attempted & (1 << index) != 0 {
        return Ok(());
    }
    child.loop_checkpoint_attempted |= 1 << index;
    let region = &REGIONS[index];
    if !guards(child, region, *registers) {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD LOOP CHECKPOINT BYPASS region={} reason=entry-guard",
                region.name
            ),
        );
        return Ok(());
    }
    let current = match pages(child) {
        Ok(pages) => pages,
        Err(error) => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD LOOP CHECKPOINT BYPASS region={} reason={error}",
                    region.name
                ),
            );
            return Ok(());
        }
    };
    let extended = context.extended_state().map_err(|e| e.to_string())?;
    let identity = identity(child);
    let cached = match async_fs::read_file(region.path).await {
        Ok(bytes) => wc3::checkpoint::decode(&bytes, region.boundary).ok(),
        Err(_) => None,
    };
    if let Some(cached) = cached.filter(|c| {
        wc3::checkpoint::matches_inputs(
            c,
            identity,
            *registers,
            *debug,
            &extended,
            current.iter().map(|p| (p.va, p.before_sha256)),
        )
    }) {
        // All validation precedes the first guest write. A write failure is fatal,
        // never a fall-back into partially restored execution.
        for page in &cached.pages {
            if let Some(after) = &page.after {
                checkpoint_write(child, page.va, after)?;
            }
        }
        context
            .set_extended_state(&cached.after_extended_state)
            .map_err(|e| e.to_string())?;
        *registers = cached.after_registers;
        *debug = cached.after_debug_registers;
        child.single_step_count = child
            .single_step_count
            .saturating_add(cached.after_single_step_count);
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD LOOP CHECKPOINT HIT region={} from=0x{:08x} to=0x{:08x} skipped_steps={} pages={}",
                region.name,
                region.boundary.from,
                region.boundary.to,
                cached.after_single_step_count,
                current.len()
            ),
        );
        return Ok(());
    }
    let before = TableCheckpoint {
        image_sha256: identity,
        table_base: 0,
        before_registers: *registers,
        before_debug_registers: *debug,
        before_extended_state: extended,
        after_registers: *registers,
        after_debug_registers: *debug,
        after_extended_state: extended,
        pages: Vec::new(),
        after_single_step_count: 0,
        after_dword_scan_watch: None,
    };
    child.loop_checkpoint_capture = Some(LoopCheckpointCapture {
        region: index,
        before,
        pages: current,
        steps: child.single_step_count,
    });
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD LOOP CHECKPOINT CAPTURE region={} from=0x{:08x} to=0x{:08x}",
            region.name, region.boundary.from, region.boundary.to
        ),
    );
    Ok(())
}

async fn finish(
    child: &PendingChild,
    context: &Context,
    registers: Registers,
    debug: DebugRegisters,
    capture: LoopCheckpointCapture,
) -> Result<(), String> {
    table_checkpoint_quiescent(child).map_err(str::to_owned)?;
    let region = &REGIONS[capture.region];
    if !hash_matches(
        child,
        region.code_start,
        (region.boundary.to - region.code_start) as usize,
        region.code_hash,
    ) || !hash_matches(child, WAR3_DIVIDE_EXCEPTION_HANDLER, 0x3bd, HASH_HANDLER)
    {
        return Err("code-changed".into());
    }
    let mut checkpoint = capture.before;
    checkpoint.after_registers = registers;
    checkpoint.after_debug_registers = debug;
    checkpoint.after_extended_state = context.extended_state().map_err(|e| e.to_string())?;
    checkpoint.after_single_step_count = child
        .single_step_count
        .checked_sub(capture.steps)
        .ok_or("step-count")?;
    for page in capture.pages {
        let mut after = vec![0; 4096];
        checkpoint_read(child, page.va, &mut after)?;
        checkpoint.pages.push(TableCheckpointPage {
            va: page.va,
            before_sha256: page.before_sha256,
            after: (after != page.before).then_some(after),
        });
    }
    let encoded = wc3::checkpoint::encode(&checkpoint, region.boundary)?;
    async_fs::create_dir_all(TABLE_CHECKPOINT_DIR)
        .await
        .map_err(|e| e.to_string())?;
    async_fs::write_file(region.path, &encoded)
        .await
        .map_err(|e| e.to_string())?;
    let readback = async_fs::read_file(region.path)
        .await
        .map_err(|e| e.to_string())?;
    if readback != encoded || wc3::checkpoint::decode(&readback, region.boundary).is_err() {
        return Err("read-back".into());
    }
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD LOOP CHECKPOINT CREATED region={} steps={} pages={} bytes={}",
            region.name,
            checkpoint.after_single_step_count,
            checkpoint.pages.len(),
            encoded.len()
        ),
    );
    Ok(())
}

const HASH_SCAN: [u8; 32] = [
    0x66, 0xbf, 0x42, 0xeb, 0x9e, 0x0b, 0x10, 0x0f, 0xc4, 0xb4, 0xdb, 0x88, 0xd9, 0x82, 0xd7, 0xb3,
    0xe8, 0x8e, 0xd4, 0xcc, 0x86, 0xc7, 0x21, 0xd3, 0xa9, 0xdf, 0x74, 0x39, 0xcc, 0x61, 0x0b, 0x70,
];

const HASH_SUM: [u8; 32] = [
    0xc7, 0x31, 0x05, 0x1d, 0xf3, 0xf0, 0xdb, 0xa4, 0x91, 0x22, 0x6f, 0xb1, 0x2b, 0xf4, 0x9d, 0x0c,
    0x07, 0xf4, 0x8a, 0x2e, 0xb4, 0x5e, 0xf8, 0xe6, 0xb9, 0xd8, 0x61, 0x63, 0x28, 0x6a, 0xff, 0xa1,
];

const HASH_HANDLER: [u8; 32] = [
    0x58, 0x82, 0xcc, 0xe6, 0x6e, 0x01, 0x0c, 0x04, 0x7c, 0x20, 0x8c, 0xd0, 0x13, 0xf4, 0xb0, 0x23,
    0xa5, 0xa8, 0xb3, 0xbf, 0x9a, 0xfe, 0xcc, 0xf1, 0xc3, 0xa5, 0xe3, 0xc7, 0x4e, 0x1d, 0x64, 0x9a,
];
