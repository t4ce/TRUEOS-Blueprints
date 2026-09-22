use super::*;

const ERROR_PROC_NOT_FOUND: u32 = 127;
const EXEC_SAMPLE_PREEMPTIONS: u64 = 8;
const MAX_SEH_CHAIN_DEPTH: u32 = 64;
const WAR3_NULL_CALL_SLOT: u32 = 0x0049_cbec;
const WAR3_NULL_CALL_NEIGHBORS: u32 = 0x0049_cbdc;
const WAR3_REPEATED_NULL_CALL_SLOT: u32 = 0x0049_a960;
const WAR3_DIVIDE_EXCEPTION_HANDLER: u32 = 0x0045_a0c0;
const WAR3_DIVIDE_EXCEPTION_EIP: u32 = 0x0045_ae47;
const WAR3_DIVIDE_TABLE_BASE_SLOT: u32 = 0x0049_ef00;
const WAR3_DIVIDE_INDEX_SLOT: u32 = 0x0049_c490;
const WAR3_DIVIDE_STATE_POINTER_SLOT: u32 = 0x0049_dc6c;
const WAR3_SCAN_INDEX: u32 = 0x0049_dc90;
const WAR3_SCAN_SOURCE: u32 = 0x0049_a594;
const WAR3_SCAN_STAGE: u32 = 0x0049_a590;
const WAR3_SCAN_CHECKSUM: u32 = 0x0049_a598;
const WAR3_SCAN_RESET: u32 = 0x0049_2614;
const WAR3_SCAN_GATE: u32 = 0x0049_a430;
const WAR3_SCAN_BOUND: u32 = 0x0049_c650;
const WAR3_SCAN_COUNT: u32 = 0x0049_a980;
const WAR3_HOTLOOP_START: u32 = 0x0045_af60;
const WAR3_SCAN_STEP_START: u32 = 0x0045_af54;
const WAR3_SCAN_STEP_END: u32 = 0x0045_b010;
const WAR3_DWORD_SCAN_INDEX: u32 = 0x0049_aa5c;
const WAR3_TABLE_FILL_BASE_SLOT: u32 = 0x0049_d024;
const WAR3_TABLE_FILL_BOUND: u32 = 0x0000_2000;
const WAR3_TABLE_FILL_VALUE: u32 = 0x0045_e2f0;
const WAR3_TABLE_FILL_STEP_START: u32 = 0x0046_1496;
const WAR3_TABLE_FILL_STEP_END: u32 = 0x0046_14c3;
const WAR3_TABLE_FILL_HEARTBEAT_STRIDE: u32 = 0x100;

const TABLE_CHECKPOINT_MAGIC: &[u8; 8] = b"WC3TFCP1";
const TABLE_CHECKPOINT_VERSION: u32 = 1;
const TABLE_CHECKPOINT_PAGE_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TableCheckpointPage {
    va: u32,
    before_sha256: [u8; 32],
    after: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TableCheckpoint {
    image_sha256: [u8; 32],
    table_base: u32,
    before_registers: Registers,
    before_debug_registers: DebugRegisters,
    before_extended_state: ExtendedState,
    after_registers: Registers,
    after_debug_registers: DebugRegisters,
    after_extended_state: ExtendedState,
    pages: Vec<TableCheckpointPage>,
    after_single_step_count: u64,
    after_dword_scan_watch: Option<DwordScanWatch>,
}

#[cfg(test)]
mod table_checkpoint_tests {
    use super::*;

    #[test]
    fn table_checkpoint_codec_rejects_tampering_and_round_trips_extended_state() {
        let mut before_extended_state = ExtendedState {
            mask: 0x7,
            bytes: [0; X86_EXTENDED_STATE_BYTES],
        };
        before_extended_state.bytes[0] = 0x37;
        let mut after_extended_state = before_extended_state;
        after_extended_state.bytes[831] = 0xa5;
        let checkpoint = TableCheckpoint {
            image_sha256: [0x11; 32],
            table_base: 0x1400_b810,
            before_registers: Registers { eip: TABLE_CHECKPOINT_FROM_EIP, eflags: 0x106, ..Registers::default() },
            before_debug_registers: DebugRegisters { dr0: 1, dr6: 0x4000, dr7: 0x403, ..DebugRegisters::default() },
            before_extended_state,
            after_registers: Registers { eip: TABLE_CHECKPOINT_TO_EIP, eflags: 0x106, ..Registers::default() },
            after_debug_registers: DebugRegisters { dr0: 1, dr6: 0x4000, dr7: 0x403, ..DebugRegisters::default() },
            after_extended_state,
            pages: vec![
                TableCheckpointPage { va: 0x0040_0000, before_sha256: [0x22; 32], after: None },
                TableCheckpointPage { va: 0x1400_b000, before_sha256: [0x33; 32], after: Some(vec![0x44; TABLE_CHECKPOINT_PAGE_BYTES]) },
            ],
            after_single_step_count: 0x2000,
            after_dword_scan_watch: Some(DwordScanWatch { last_heartbeat_index: Some(0x2000) }),
        };
        let encoded = checkpoint_encode(&checkpoint).unwrap();
        let decoded = checkpoint_decode(&encoded).unwrap();
        assert_eq!(decoded, checkpoint);

        let mut tampered = encoded;
        tampered[20] ^= 1;
        assert_eq!(checkpoint_decode(&tampered), Err("table checkpoint checksum".into()));
    }
}

fn checkpoint_sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn checkpoint_read(child: &PendingChild, va: u32, bytes: &mut [u8]) -> Result<(), String> {
    let read = child
        .address_space
        .read(va, bytes)
        .map_err(|error| format!("table checkpoint read 0x{va:08x}: {error}"))?;
    if read == bytes.len() {
        Ok(())
    } else {
        Err(format!("table checkpoint short read 0x{va:08x}: {read}/{}", bytes.len()))
    }
}

fn checkpoint_write(child: &mut PendingChild, va: u32, bytes: &[u8]) -> Result<(), String> {
    let written = child
        .address_space
        .write(va, bytes)
        .map_err(|error| format!("table checkpoint write 0x{va:08x}: {error}"))?;
    if written == bytes.len() {
        Ok(())
    } else {
        Err(format!("table checkpoint short write 0x{va:08x}: {written}/{}", bytes.len()))
    }
}

fn checkpoint_capture_pages(
    child: &PendingChild,
    ranges: &[(u32, u32)],
) -> Result<Vec<CachedPage>, String> {
    let mut pages = std::collections::BTreeMap::new();
    for &(start, len) in ranges {
        let end = start
            .checked_add(len)
            .ok_or("table checkpoint range overflow")?;
        let mut va = start & !0xfff;
        let page_end = end
            .checked_add(0xfff)
            .ok_or("table checkpoint page range overflow")?
            & !0xfff;
        while va < page_end {
            let mut before = vec![0; TABLE_CHECKPOINT_PAGE_BYTES];
            checkpoint_read(child, va, &mut before)?;
            pages.entry(va).or_insert_with(|| CachedPage {
                va,
                before_sha256: checkpoint_sha256(&before),
                before,
            });
            va = va.checked_add(TABLE_CHECKPOINT_PAGE_BYTES as u32).ok_or("table checkpoint VA overflow")?;
        }
    }
    Ok(pages.into_values().collect())
}

fn checkpoint_put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn checkpoint_put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn checkpoint_put_registers(out: &mut Vec<u8>, registers: Registers) {
    for value in [
        registers.eax,
        registers.ebx,
        registers.ecx,
        registers.edx,
        registers.esi,
        registers.edi,
        registers.ebp,
        registers.esp,
        registers.eip,
        registers.eflags,
        registers.fs_base,
    ] {
        checkpoint_put_u32(out, value);
    }
}

fn checkpoint_put_debug_registers(out: &mut Vec<u8>, registers: DebugRegisters) {
    for value in [
        registers.dr0,
        registers.dr1,
        registers.dr2,
        registers.dr3,
        registers.dr6,
        registers.dr7,
    ] {
        checkpoint_put_u32(out, value);
    }
}

fn checkpoint_put_extended_state(out: &mut Vec<u8>, state: &ExtendedState) {
    checkpoint_put_u64(out, state.mask);
    out.extend_from_slice(&state.bytes);
}

fn checkpoint_encode(checkpoint: &TableCheckpoint) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.extend_from_slice(TABLE_CHECKPOINT_MAGIC);
    checkpoint_put_u32(&mut out, TABLE_CHECKPOINT_VERSION);
    out.extend_from_slice(&checkpoint.image_sha256);
    checkpoint_put_u32(&mut out, TABLE_CHECKPOINT_FROM_EIP);
    checkpoint_put_u32(&mut out, TABLE_CHECKPOINT_TO_EIP);
    checkpoint_put_u32(&mut out, checkpoint.table_base);
    checkpoint_put_u32(&mut out, TABLE_CHECKPOINT_TABLE_BYTES);
    checkpoint_put_registers(&mut out, checkpoint.before_registers);
    checkpoint_put_debug_registers(&mut out, checkpoint.before_debug_registers);
    checkpoint_put_extended_state(&mut out, &checkpoint.before_extended_state);
    checkpoint_put_registers(&mut out, checkpoint.after_registers);
    checkpoint_put_debug_registers(&mut out, checkpoint.after_debug_registers);
    checkpoint_put_extended_state(&mut out, &checkpoint.after_extended_state);
    checkpoint_put_u64(&mut out, checkpoint.after_single_step_count);
    match checkpoint.after_dword_scan_watch {
        Some(watch) => {
            out.push(1);
            checkpoint_put_u32(&mut out, watch.last_heartbeat_index.unwrap_or(u32::MAX));
        }
        None => out.push(0),
    }
    checkpoint_put_u32(
        &mut out,
        u32::try_from(checkpoint.pages.len()).map_err(|_| "table checkpoint page count")?,
    );
    for page in &checkpoint.pages {
        checkpoint_put_u32(&mut out, page.va);
        out.extend_from_slice(&page.before_sha256);
        match &page.after {
            Some(after) => {
                if after.len() != TABLE_CHECKPOINT_PAGE_BYTES {
                    return Err("table checkpoint changed page length".into());
                }
                out.push(1);
                out.extend_from_slice(after);
            }
            None => out.push(0),
        }
    }
    let digest = checkpoint_sha256(&out);
    out.extend_from_slice(&digest);
    Ok(out)
}

struct CheckpointReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CheckpointReader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.offset.checked_add(len).ok_or("table checkpoint decode overflow")?;
        let slice = self.bytes.get(self.offset..end).ok_or("table checkpoint truncated")?;
        self.offset = end;
        Ok(slice)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.take(N)?
            .try_into()
            .map_err(|_| "table checkpoint array".to_owned())
    }
}

fn checkpoint_get_registers(input: &mut CheckpointReader<'_>) -> Result<Registers, String> {
    Ok(Registers {
        eax: input.u32()?, ebx: input.u32()?, ecx: input.u32()?, edx: input.u32()?,
        esi: input.u32()?, edi: input.u32()?, ebp: input.u32()?, esp: input.u32()?,
        eip: input.u32()?, eflags: input.u32()?, fs_base: input.u32()?,
        ..Registers::default()
    })
}

fn checkpoint_get_debug_registers(input: &mut CheckpointReader<'_>) -> Result<DebugRegisters, String> {
    Ok(DebugRegisters {
        dr0: input.u32()?, dr1: input.u32()?, dr2: input.u32()?, dr3: input.u32()?,
        dr6: input.u32()?, dr7: input.u32()?,
    })
}

fn checkpoint_get_extended_state(input: &mut CheckpointReader<'_>) -> Result<ExtendedState, String> {
    Ok(ExtendedState {
        mask: input.u64()?,
        bytes: input.array::<X86_EXTENDED_STATE_BYTES>()?,
    })
}

fn checkpoint_decode(bytes: &[u8]) -> Result<TableCheckpoint, String> {
    if bytes.len() < TABLE_CHECKPOINT_MAGIC.len() + 32 {
        return Err("table checkpoint truncated".into());
    }
    let (body, trailing) = bytes.split_at(bytes.len() - 32);
    if checkpoint_sha256(body) != <[u8; 32]>::try_from(trailing).unwrap() {
        return Err("table checkpoint checksum".into());
    }
    let mut input = CheckpointReader { bytes: body, offset: 0 };
    if input.array::<8>()? != *TABLE_CHECKPOINT_MAGIC || input.u32()? != TABLE_CHECKPOINT_VERSION {
        return Err("table checkpoint format".into());
    }
    let image_sha256 = input.array::<32>()?;
    if input.u32()? != TABLE_CHECKPOINT_FROM_EIP || input.u32()? != TABLE_CHECKPOINT_TO_EIP {
        return Err("table checkpoint boundaries".into());
    }
    let table_base = input.u32()?;
    if input.u32()? != TABLE_CHECKPOINT_TABLE_BYTES {
        return Err("table checkpoint table bytes".into());
    }
    let before_registers = checkpoint_get_registers(&mut input)?;
    let before_debug_registers = checkpoint_get_debug_registers(&mut input)?;
    let before_extended_state = checkpoint_get_extended_state(&mut input)?;
    let after_registers = checkpoint_get_registers(&mut input)?;
    let after_debug_registers = checkpoint_get_debug_registers(&mut input)?;
    let after_extended_state = checkpoint_get_extended_state(&mut input)?;
    if before_registers.eip != TABLE_CHECKPOINT_FROM_EIP
        || after_registers.eip != TABLE_CHECKPOINT_TO_EIP
    {
        return Err("table checkpoint register boundaries".into());
    }
    let after_single_step_count = input.u64()?;
    let after_dword_scan_watch = match input.take(1)?[0] {
        0 => None,
        1 => Some(DwordScanWatch { last_heartbeat_index: match input.u32()? { u32::MAX => None, value => Some(value) } }),
        _ => return Err("table checkpoint dword watch".into()),
    };
    let page_count = usize::try_from(input.u32()?).map_err(|_| "table checkpoint page count")?;
    if page_count > 4096 {
        return Err("table checkpoint excessive pages".into());
    }
    let mut pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        let va = input.u32()?;
        let before_sha256 = input.array::<32>()?;
        let after = match input.take(1)?[0] {
            0 => None,
            1 => Some(input.take(TABLE_CHECKPOINT_PAGE_BYTES)?.to_vec()),
            _ => return Err("table checkpoint changed-page marker".into()),
        };
        pages.push(TableCheckpointPage { va, before_sha256, after });
    }
    if input.offset != body.len() {
        return Err("table checkpoint trailing body".into());
    }
    Ok(TableCheckpoint {
        image_sha256, table_base, before_registers, before_debug_registers,
        before_extended_state, after_registers, after_debug_registers,
        after_extended_state, pages, after_single_step_count, after_dword_scan_watch,
    })
}

fn table_checkpoint_quiescent(child: &PendingChild) -> Result<(), &'static str> {
    if child.execution != ChildExecutionState::ImageEntryRunning {
        return Err("execution");
    }
    if child.seh.is_some() {
        return Err("pending-seh");
    }
    if child.unhandled_filter_call.is_some() {
        return Err("pending-uef");
    }
    if child.initterm.is_some() {
        return Err("initterm");
    }
    if child.cipow.is_some() {
        return Err("cipow");
    }
    Ok(())
}

fn table_checkpoint_table_base(child: &PendingChild) -> Result<u32, String> {
    child_read_u32(child, WAR3_TABLE_FILL_BASE_SLOT)
        .filter(|base| *base != 0)
        .ok_or_else(|| "table checkpoint table base unavailable".into())
}

fn table_checkpoint_host_state(
    session: &Wc3Session,
    pid: u32,
) -> Result<(u32, usize, usize, usize), String> {
    let process = session
        .process(pid)
        .ok_or_else(|| "table checkpoint child process missing".to_owned())?;
    Ok((
        process.xp.call_count,
        process.xp.provider_import_count(),
        session.objects.len(),
        process.handles.len(),
    ))
}

fn begin_table_checkpoint_capture(
    child: &mut PendingChild,
    context: &Context,
    restored: Registers,
    restored_debug: DebugRegisters,
    session: &Wc3Session,
) -> Result<(), String> {
    table_checkpoint_quiescent(child).map_err(str::to_owned)?;
    let (call_count, provider_import_count, session_object_count, process_handle_count) =
        table_checkpoint_host_state(session, child.pid)?;
    let table_base = table_checkpoint_table_base(child)?;
    let image_bytes = u32::try_from(child.image.image.len()).map_err(|_| "table checkpoint image size")?;
    let teb = thread_teb_va(child.tid)?;
    let pages = checkpoint_capture_pages(
        child,
        &[
            (child.image.image_base, image_bytes),
            (PROCESS_DATA_VA, 0x1000),
            (teb, 0x1000),
            (STACK_BASE, STACK_BYTES as u32),
            (table_base, TABLE_CHECKPOINT_TABLE_BYTES),
        ],
    )?;
    child.table_checkpoint_capture = Some(TableCheckpointCapture {
        table_base,
        before_registers: restored,
        before_debug_registers: restored_debug,
        before_extended_state: context.extended_state().map_err(|error| error.to_string())?,
        pages,
        call_count,
        provider_import_count,
        session_object_count,
        process_handle_count,
        provider_thunk_bytes: child.provider_thunk_bytes,
        crt_heap_mapped_end: child.crt_heap_mapped_end,
        win_heap_mapped_end: child.win_heap_mapped_end,
    });
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT CAPTURE entry=0x{:08x} index=0 table=0x{:08x} pages={} call_count={}",
            restored.eip,
            table_base,
            child.table_checkpoint_capture.as_ref().unwrap().pages.len(),
            call_count,
        ),
    );
    Ok(())
}

async fn finish_table_checkpoint_capture(
    child: &mut PendingChild,
    context: &Context,
    restored: Registers,
    restored_debug: DebugRegisters,
    session: &Wc3Session,
) -> Result<(), String> {
    table_checkpoint_quiescent(child).map_err(str::to_owned)?;
    let capture = child
        .table_checkpoint_capture
        .take()
        .ok_or_else(|| "table checkpoint capture missing".to_owned())?;
    if child_read_u32(child, WAR3_DWORD_SCAN_INDEX) != Some(WAR3_TABLE_FILL_BOUND) {
        return Err("table checkpoint exit index".into());
    }
    if table_checkpoint_table_base(child)? != capture.table_base {
        return Err("table checkpoint table base changed".into());
    }
    let (call_count, provider_import_count, session_object_count, process_handle_count) =
        table_checkpoint_host_state(session, child.pid)?;
    if child.execution != ChildExecutionState::ImageEntryRunning
        || call_count != capture.call_count
        || provider_import_count != capture.provider_import_count
        || session_object_count != capture.session_object_count
        || process_handle_count != capture.process_handle_count
        || child.provider_thunk_bytes != capture.provider_thunk_bytes
        || child.crt_heap_mapped_end != capture.crt_heap_mapped_end
        || child.win_heap_mapped_end != capture.win_heap_mapped_end
    {
        return Err("table checkpoint purity invariant".into());
    }
    let mut pages = Vec::with_capacity(capture.pages.len());
    for page in capture.pages {
        let mut after = vec![0; page.before.len()];
        checkpoint_read(child, page.va, &mut after)?;
        pages.push(TableCheckpointPage {
            va: page.va,
            before_sha256: page.before_sha256,
            after: (after != page.before).then_some(after),
        });
    }
    let checkpoint = TableCheckpoint {
        image_sha256: checkpoint_sha256(child.self_image_bytes.as_slice()),
        table_base: capture.table_base,
        before_registers: capture.before_registers,
        before_debug_registers: capture.before_debug_registers,
        before_extended_state: capture.before_extended_state,
        after_registers: restored,
        after_debug_registers: restored_debug,
        after_extended_state: context.extended_state().map_err(|error| error.to_string())?,
        pages,
        after_single_step_count: child.single_step_count,
        after_dword_scan_watch: child.dword_scan_watch,
    };
    let encoded = checkpoint_encode(&checkpoint)?;
    async_fs::create_dir_all(TABLE_CHECKPOINT_DIR)
        .await
        .map_err(|error| format!("create table checkpoint directory: {error}"))?;
    async_fs::write_file(TABLE_CHECKPOINT_PATH, &encoded)
        .await
        .map_err(|error| format!("write table checkpoint: {error}"))?;
    let readback = async_fs::read_file(TABLE_CHECKPOINT_PATH)
        .await
        .map_err(|error| format!("read back table checkpoint: {error}"))?;
    if readback != encoded || checkpoint_decode(&readback).is_err() {
        return Err("table checkpoint read-back verification".into());
    }
    let changed = checkpoint.pages.iter().filter(|page| page.after.is_some()).count();
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT CREATED table=0x{:08x} pages={} changed={} bytes={}",
            checkpoint.table_base,
            checkpoint.pages.len(),
            changed,
            encoded.len(),
        ),
    );
    Ok(())
}

async fn try_restore_table_checkpoint(
    child: &mut PendingChild,
    context: &mut Context,
    restored: &mut Registers,
    restored_debug: &mut DebugRegisters,
) -> Result<bool, String> {
    let bytes = match async_fs::read_file(TABLE_CHECKPOINT_PATH).await {
        Ok(bytes) => bytes,
        Err(error) => {
            logl::log(level::IMPORTANT, format_args!("WC3 CHILD TABLE CHECKPOINT BYPASS reason=cache-unavailable error={error}"));
            return Ok(false);
        }
    };
    let checkpoint = match checkpoint_decode(&bytes) {
        Ok(checkpoint) => checkpoint,
        Err(reason) => {
            logl::log(level::IMPORTANT, format_args!("WC3 CHILD TABLE CHECKPOINT BYPASS reason={reason}"));
            return Ok(false);
        }
    };
    let bypass = |reason: &str| {
        logl::log(level::IMPORTANT, format_args!("WC3 CHILD TABLE CHECKPOINT BYPASS reason={reason}"));
        false
    };
    if table_checkpoint_quiescent(child).is_err() { return Ok(bypass("not-quiescent")); }
    if checkpoint.image_sha256 != checkpoint_sha256(child.self_image_bytes.as_slice()) { return Ok(bypass("image-sha256")); }
    if *restored != checkpoint.before_registers { return Ok(bypass("registers")); }
    if *restored_debug != checkpoint.before_debug_registers { return Ok(bypass("debug-registers")); }
    if context.extended_state().map_err(|error| error.to_string())? != checkpoint.before_extended_state { return Ok(bypass("extended-state")); }
    if table_checkpoint_table_base(child)? != checkpoint.table_base { return Ok(bypass("table-base")); }
    for page in &checkpoint.pages {
        let mut current = vec![0; TABLE_CHECKPOINT_PAGE_BYTES];
        checkpoint_read(child, page.va, &mut current)?;
        if checkpoint_sha256(&current) != page.before_sha256 {
            return Ok(bypass("precondition-page"));
        }
    }
    for page in &checkpoint.pages {
        if let Some(after) = &page.after {
            checkpoint_write(child, page.va, after)?;
        }
    }
    *restored = checkpoint.after_registers;
    *restored_debug = checkpoint.after_debug_registers;
    context
        .set_extended_state(&checkpoint.after_extended_state)
        .map_err(|error| error.to_string())?;
    child.single_step_count = checkpoint.after_single_step_count;
    child.dword_scan_watch = checkpoint.after_dword_scan_watch;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT HIT from=0x{:08x} to=0x{:08x} table=0x{:08x} pages={}",
            TABLE_CHECKPOINT_FROM_EIP,
            TABLE_CHECKPOINT_TO_EIP,
            checkpoint.table_base,
            checkpoint.pages.len(),
        ),
    );
    Ok(true)
}

fn war3_scan_single_step(exception: ChildException, registers: Registers) -> bool {
    exception.vector == Some(1)
        && exception.debug_status == Some(0x0000_4000)
        && (WAR3_SCAN_STEP_START..=WAR3_SCAN_STEP_END).contains(&registers.eip)
}

pub(super) fn war3_dword_scan_single_step(exception: ChildException, registers: Registers) -> bool {
    exception.vector == Some(1)
        && exception.debug_status.is_some_and(|dr6| dr6 & 0x4000 != 0)
        && (WAR3_TABLE_FILL_STEP_START..=WAR3_TABLE_FILL_STEP_END).contains(&registers.eip)
}

fn quiet_war3_exception(exception: ChildException, registers: Registers) -> bool {
    (exception.vector == Some(0) && registers.eip == WAR3_DIVIDE_EXCEPTION_EIP)
        || (exception.vector == Some(14)
            && registers.eip == 0x0045_af51
            && exception.fault_linear == Some(0)
            && exception.error.is_some_and(|error| error & 2 != 0))
}

pub(super) fn boring_war3_single_step(
    exception: ChildException,
    registers: Registers,
    handler: u32,
) -> bool {
    exception.vector == Some(1)
        && exception
            .debug_status
            .is_some_and(|dr6| dr6 & 0x0000_e00f == 0x0000_4000)
        && registers.eip != 0
        && handler == WAR3_DIVIDE_EXCEPTION_HANDLER
}

fn current_child_seh_handler(child: &PendingChild, fs_base: u32) -> Option<u32> {
    let mut head = [0; 4];
    if child.address_space.read(fs_base, &mut head).ok()? != head.len() {
        return None;
    }
    let head = u32::from_le_bytes(head);
    (head != u32::MAX)
        .then(|| {
            read_seh_registration(&child.address_space, head)
                .ok()
                .map(|registration| registration.handler)
        })
        .flatten()
}

fn child_image_import_at_rva(
    child: &PendingChild,
    rva: u32,
) -> Option<&pe32::ImportDescriptor> {
    child.image.imports.iter().find(|import| import.iat_rva == rva)
}

fn import_symbol_diagnostic(symbol: &pe32::ImportSymbol) -> String {
    match symbol {
        pe32::ImportSymbol::Name(name) => format!("\"{name}\""),
        pe32::ImportSymbol::Ordinal(ordinal) => format!("ordinal:{ordinal}"),
    }
}

fn log_null_slot_provenance(child: &PendingChild, slot: u32, runtime_value: Option<u32>) {
    let Some(("War3.exe", rva)) = child_pc_owner(child, slot) else {
        return;
    };
    let runtime_value = runtime_value
        .map(|value| format!("0x{value:08x}"))
        .unwrap_or_else(|| "<unreadable>".into());
    let Some(import) = child_image_import_at_rva(child, rva) else {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD NULL SLOT PROVENANCE slot=0x{slot:08x} rva=0x{rva:08x} kind=war3-runtime-pointer runtime_value={runtime_value}",
            ),
        );
        return;
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL SLOT PROVENANCE slot=0x{slot:08x} rva=0x{rva:08x} kind=pe-import module=\"{}\" symbol={} runtime_value={runtime_value}",
            import.module,
            import_symbol_diagnostic(&import.symbol),
        ),
    );
    let (category, normal_path) = if import.module.eq_ignore_ascii_case("Storm.dll") {
        ("storm-export", "War3-native-export-bind")
    } else if import.module.eq_ignore_ascii_case("Mss32.dll") {
        ("mss-export", "War3-native-export-bind")
    } else {
        ("external-provider", "child_loader-provider-thunk-bind")
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL SLOT BINDING slot=0x{slot:08x} category={category} normal_path={normal_path} reason=slot-zero-after-loader",
        ),
    );
}

fn log_child_entry_null_slot_check(child: &PendingChild) {
    let mut slot_bytes = [0; 4];
    let slot_value = (child
        .address_space
        .read(WAR3_NULL_CALL_SLOT, &mut slot_bytes)
        .ok()
        == Some(slot_bytes.len()))
    .then(|| u32::from_le_bytes(slot_bytes));
    let is_import = WAR3_NULL_CALL_SLOT
        .checked_sub(child.image.image_base)
        .is_some_and(|rva| child_image_import_at_rva(child, rva).is_some());
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD ENTRY SLOT CHECK slot=0x{WAR3_NULL_CALL_SLOT:08x} value={} import={}",
            slot_value
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "<unreadable>".into()),
            u32::from(is_import),
        ),
    );
    let mut table = [0; 32];
    let words = if child
        .address_space
        .read(WAR3_NULL_CALL_NEIGHBORS, &mut table)
        .ok()
        == Some(table.len())
    {
        table
            .chunks_exact(4)
            .map(|word| format!("0x{:08x}", u32::from_le_bytes(word.try_into().unwrap())))
            .collect::<Vec<_>>()
            .join(",")
    } else {
        "<unreadable>".into()
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD ENTRY SLOT NEIGHBORS base=0x{WAR3_NULL_CALL_NEIGHBORS:08x} words=[{words}]",
        ),
    );
}

fn log_child_slot_xrefs(child: &PendingChild, slot: u32) {
    let needle = slot.to_le_bytes();
    let mut matches = 0u32;
    for (offset, bytes) in child.image.image.windows(needle.len()).enumerate() {
        if bytes != needle {
            continue;
        }
        let Ok(rva) = u32::try_from(offset) else {
            break;
        };
        let Some(site) = child.image.image_base.checked_add(rva) else {
            break;
        };
        let context_start = offset.saturating_sub(16);
        let context_end = offset
            .saturating_add(needle.len())
            .saturating_add(16)
            .min(child.image.image.len());
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SLOT XREF slot=0x{slot:08x} site=0x{site:08x} rva=0x{rva:08x} bytes=\"{}\"",
                diagnostic_hex_bytes(&child.image.image[context_start..context_end]),
            ),
        );
        matches = matches.saturating_add(1);
    }
    if matches == 0 {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SLOT XREF slot=0x{slot:08x} matches=0",
            ),
        );
    }
}

fn should_log_execution_sample(count: u64) -> bool {
    count == 1 || count % EXEC_SAMPLE_PREEMPTIONS == 0
}

fn service_sync_request(
    session: &mut Wc3Session,
    caller_pid: u32,
    request: SessionRequest,
    contexts: &mut [GuestContext],
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<u32, String> {
    match request {
        SessionRequest::CreateEvent(request) => {
            let (handle, already_exists) = session.create_event(caller_pid, request);
            session
                .process_mut(caller_pid)
                .ok_or_else(|| "event process missing".to_owned())?
                .xp
                .set_last_error(if already_exists { 183 } else { 0 });
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD EVENT CREATE pid={} handle=0x{:08x} already_exists={}",
                    caller_pid,
                    handle,
                    already_exists as u8,
                ),
            );
            Ok(handle)
        }
        SessionRequest::CreateMutex { key, request } => {
            let (handle, error, existed) = match session.create_mutex(key, request) {
                Ok((handle, existed)) => (handle, if existed { 183 } else { 0 }, existed),
                Err(error) => (0, error, false),
            };
            session.process_mut(key.pid)
                .ok_or_else(|| "mutex process missing".to_owned())?
                .xp.set_last_error(error);
            logl::log(level::IMPORTANT, format_args!(
                "WC3 CHILD MUTEX CREATE pid={} tid={} handle=0x{:08x} already_exists={} error={}",
                key.pid, key.tid, handle, existed as u8, error
            ));
            Ok(handle)
        }
        SessionRequest::ReleaseMutex { key, handle } => {
            match session.release_mutex(key, handle) {
                Ok(woken) => {
                    resume_completed_waiters(&woken, "release-mutex", contexts, wait_deadlines)?;
                    logl::log(level::IMPORTANT, format_args!(
                        "WC3 CHILD MUTEX RELEASE pid={} tid={} handle=0x{:08x} waiters_woken={} result=1",
                        key.pid, key.tid, handle, woken.len()
                    ));
                    Ok(1)
                }
                Err(error) => {
                    session.process_mut(key.pid)
                        .ok_or_else(|| "mutex process missing".to_owned())?
                        .xp.set_last_error(error);
                    Ok(0)
                }
            }
        }
        SessionRequest::CloseHandle { pid, handle } => {
            if session.close_handle(pid, handle) {
                Ok(1)
            } else {
                session.process_mut(pid)
                    .ok_or_else(|| "CloseHandle process missing".to_owned())?
                    .xp.set_last_error(6);
                Ok(0)
            }
        }
        _ => Err("unexpected synchronization request".into()),
    }
}

fn resume_completed_waiters(
    woken: &[CompletedWait],
    reason: &str,
    contexts: &mut [GuestContext],
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<(), String> {
    for completed in woken {
        let request = &completed.request;
        let index = context_index(contexts, request.key)
            .ok_or_else(|| format!("{reason} waiter context missing"))?;
        let mut registers = wait_deadlines
            .remove(&request.key)
            .map(|wait| wait.resume_registers)
            .unwrap_or(
                contexts[index]
                    .context
                    .registers()
                    .map_err(|error| error.to_string())?,
            );
        registers.eax = completed.result;
        contexts[index]
            .context
            .set_registers(registers)
            .map_err(|error| error.to_string())?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 WAIT SIGNALED pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} index={} reason={} result=0x{:08x}",
                request.key.pid,
                request.key.tid,
                request.handles[0],
                request.handles[1],
                request.count,
                request.wait_all,
                completed.result.saturating_sub(WAIT_OBJECT_0),
                reason,
                completed.result,
            ),
        );
    }
    Ok(())
}

fn terminate_launcher_thread(
    session: &mut Wc3Session,
    contexts: &mut Vec<GuestContext>,
    active: usize,
    exit_code: u32,
    thread_calls: &mut HashMap<(u32, u32), u32>,
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<Option<usize>, String> {
    let key = contexts
        .get(active)
        .ok_or_else(|| "ExitThread active context missing".to_owned())?
        .key();
    if key.pid != LAUNCHER_PID || key.tid == LAUNCHER_TID {
        return Err("ExitThread launcher-primary-thread frontier".into());
    }
    contexts.remove(active);
    session
        .launcher_mut()
        .xp
        .exit_thread(key.tid, exit_code)
        .map_err(str::to_owned)?;
    let woken = session.signal_thread(key, exit_code).map_err(str::to_owned)?;
    wait_deadlines.remove(&key);
    thread_calls.remove(&(key.pid, key.tid));
    resume_completed_waiters(&woken, "thread-exit", contexts, wait_deadlines)?;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 EXITTHREAD pid={} tid={} exit_code=0x{:08x} contexts_removed=1 waiters_woken={} result=terminated",
            key.pid,
            key.tid,
            exit_code,
            woken.len(),
        ),
    );
    if contexts.is_empty() {
        return Ok(None);
    }
    Ok(pop_runnable_context(session, contexts).or(Some(active % contexts.len())))
}

fn child_pc_owner(child: &PendingChild, eip: u32) -> Option<(&str, u32)> {
    let in_image = |base: u32, size: u32| {
        base.checked_add(size)
            .filter(|end| base <= eip && eip < *end)
            .map(|_| eip - base)
    };
    if let Some(rva) = in_image(child.image.image_base, child.image.size_of_image) {
        return Some(("War3.exe", rva));
    }
    for module in &child.native_modules {
        if let Some(rva) = in_image(module.image.image_base, module.image.size_of_image) {
            return Some((module.stored.as_str(), rva));
        }
    }
    let provider_end = thunk32::THUNK_BASE.checked_add(child.provider_thunk_bytes as u32)?;
    if thunk32::THUNK_BASE <= eip && eip < provider_end {
        return Some(("provider-thunks", eip - thunk32::THUNK_BASE));
    }
    let control_end = thunk32::CHILD_CONTROL_BASE.checked_add(0x1000)?;
    if thunk32::CHILD_CONTROL_BASE <= eip && eip < control_end {
        return Some(("child-control", eip - thunk32::CHILD_CONTROL_BASE));
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NullCallSource {
    Register {
        name: &'static str,
        target: u32,
    },
    AbsoluteMemory {
        slot: u32,
        target: Option<u32>,
    },
    RegisterMemory {
        name: &'static str,
        displacement: i32,
        slot: u32,
        target: Option<u32>,
    },
    Unknown,
}

fn register_value(registers: Registers, index: u8) -> (&'static str, u32) {
    match index {
        0 => ("eax", registers.eax),
        1 => ("ecx", registers.ecx),
        2 => ("edx", registers.edx),
        3 => ("ebx", registers.ebx),
        4 => ("esp", registers.esp),
        5 => ("ebp", registers.ebp),
        6 => ("esi", registers.esi),
        7 => ("edi", registers.edi),
        _ => unreachable!("x86 register index is three bits"),
    }
}

pub(super) fn classify_null_call_source(
    bytes: &[u8],
    registers: Registers,
    read_word: impl Fn(u32) -> Option<u32>,
) -> NullCallSource {
    let (modrm, displacement) = if let Some(instruction) = bytes.get(bytes.len().saturating_sub(2)..)
        && instruction.len() == 2
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 3
    {
        (instruction[1], 0)
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(6)..)
        && instruction.len() == 6
        && instruction[0] == 0xff
        && (instruction[1] == 0x15 || instruction[1] >> 6 == 2)
    {
        let modrm = instruction[1];
        if modrm == 0x15 {
            let slot = u32::from_le_bytes(instruction[2..6].try_into().unwrap());
            return NullCallSource::AbsoluteMemory {
                slot,
                target: read_word(slot),
            };
        }
        (modrm, i32::from_le_bytes(instruction[2..6].try_into().unwrap()))
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(3)..)
        && instruction.len() == 3
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 1
    {
        (instruction[1], i32::from(instruction[2] as i8))
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(2)..)
        && instruction.len() == 2
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 0
    {
        (instruction[1], 0)
    } else {
        return NullCallSource::Unknown;
    };
    if (modrm >> 3) & 7 != 2 {
        return NullCallSource::Unknown;
    }
    let mode = modrm >> 6;
    let base = modrm & 7;
    if mode == 3 {
        let (name, target) = register_value(registers, base);
        return NullCallSource::Register { name, target };
    }

    // ModRM base 4 has a SIB byte, while mod=00/base=5 is absolute addressing.
    if base == 4 || (mode == 0 && base == 5) {
        return NullCallSource::Unknown;
    }
    let (name, value) = register_value(registers, base);
    let slot = value.wrapping_add_signed(displacement);
    NullCallSource::RegisterMemory {
        name,
        displacement,
        slot,
        target: read_word(slot),
    }
}

fn child_fault_stack_return(child: &PendingChild, registers: Registers) -> Option<u32> {
    let mut bytes = [0; 4];
    (child.address_space.read(registers.esp, &mut bytes).ok()? == bytes.len())
        .then(|| u32::from_le_bytes(bytes))
}

fn code_before_return(address_space: &AddressSpace, return_address: u32) -> Option<[u8; 16]> {
    let start = return_address.checked_sub(16)?;
    let mut bytes = [0; 16];
    (address_space.read(start, &mut bytes).ok()? == bytes.len()).then_some(bytes)
}

fn diagnostic_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn child_read_u32(child: &PendingChild, address: u32) -> Option<u32> {
    let mut bytes = [0; 4];
    (child.address_space.read(address, &mut bytes).ok()? == bytes.len())
        .then(|| u32::from_le_bytes(bytes))
}

fn child_read_u16(child: &PendingChild, address: u32) -> Option<u16> {
    let mut bytes = [0; 2];
    (child.address_space.read(address, &mut bytes).ok()? == bytes.len())
        .then(|| u16::from_le_bytes(bytes))
}

fn child_read_u8(child: &PendingChild, address: u32) -> Option<u8> {
    let mut byte = [0; 1];
    (child.address_space.read(address, &mut byte).ok()? == byte.len()).then_some(byte[0])
}

fn log_war3_scan_progress(
    child: &mut PendingChild,
    eip: u32,
    registers: Registers,
    debug: DebugRegisters,
) {
    let progress = ScanProgress {
        stage: child_read_u8(child, WAR3_SCAN_STAGE),
        source: child_read_u32(child, WAR3_SCAN_SOURCE),
        checksum: child_read_u8(child, WAR3_SCAN_CHECKSUM),
        index: child_read_u32(child, WAR3_SCAN_INDEX),
        bound: child_read_u32(child, WAR3_SCAN_BOUND),
        reset: child_read_u32(child, WAR3_SCAN_RESET),
        gate: child_read_u32(child, WAR3_SCAN_GATE),
        tf: registers.eflags & wc3::seh::X86_EFLAGS_TF != 0,
        dr7: debug.dr7,
    };
    let previous = child.scan_progress;
    child.scan_progress = Some(progress);
    let reset = previous.is_some_and(|previous| {
        previous.source.zip(progress.source).is_some_and(|(old, new)| new < old)
            || previous.index.zip(progress.index).is_some_and(|(old, new)| new < old)
    });
    let debug_transition = previous.is_some_and(|previous| {
        previous.tf != progress.tf || previous.dr7 != progress.dr7
    });
    let heartbeat = progress.source.is_some_and(|source| {
        source & 0xff == 0 && child.scan_heartbeat_source != Some(source)
    });
    if !heartbeat && !reset && !debug_transition {
        return;
    }
    if heartbeat {
        child.scan_heartbeat_source = progress.source;
    }
    let reason = if reset {
        "reset"
    } else if debug_transition {
        "debug-transition"
    } else {
        "source-boundary"
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD SCAN HEARTBEAT reason={} eip=0x{:08x} index={} source={} bound={} stage={} dr7=0x{:08x} tf={} checksum={} gate={} reset={} dr6=0x{:08x}",
            reason,
            eip,
            progress.index.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
            progress.source.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
            progress.bound.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
            progress.stage.map(|value| format!("0x{value:02x}")).unwrap_or_else(|| "-".into()),
            debug.dr7,
            u32::from(progress.tf),
            progress.checksum.map(|value| format!("0x{value:02x}")).unwrap_or_else(|| "-".into()),
            progress.gate.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
            progress.reset.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
            debug.dr6,
        ),
    );
}

fn observe_war3_dword_scan(child: &mut PendingChild, eip: u32) {
    let Some(index) = child_read_u32(child, WAR3_DWORD_SCAN_INDEX) else {
        return;
    };
    let watch = child.dword_scan_watch.get_or_insert(DwordScanWatch {
        last_heartbeat_index: None,
    });
    if index % WAR3_TABLE_FILL_HEARTBEAT_STRIDE != 0
        || watch.last_heartbeat_index == Some(index)
    {
        return;
    }
    watch.last_heartbeat_index = Some(index);
    let base = child_read_u32(child, WAR3_TABLE_FILL_BASE_SLOT);
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE FILL eip=0x{:08x} index=0x{:04x} bound=0x{:04x} base={} value=0x{WAR3_TABLE_FILL_VALUE:08x}",
            eip,
            index,
            WAR3_TABLE_FILL_BOUND,
            base.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
        ),
    );
}

fn divide_loop_progress(child: &PendingChild) -> Option<DivideLoopProgress> {
    let table_base = child_read_u32(child, WAR3_DIVIDE_TABLE_BASE_SLOT)?;
    let index = child_read_u32(child, WAR3_DIVIDE_INDEX_SLOT)?;
    let state_ptr = child_read_u32(child, WAR3_DIVIDE_STATE_POINTER_SLOT)?;
    let input = child_read_u8(child, table_base.checked_add(index)?)?;
    let accumulator = child_read_u16(child, state_ptr)?;
    Some(DivideLoopProgress {
        table_base,
        index,
        state_ptr,
        input,
        accumulator,
    })
}

fn log_null_call_diagnostic(
    child: &PendingChild,
    pid: u32,
    tid: u32,
    registers: Registers,
) -> Option<NullLoopSignature> {
    let Some(return_address) = child_fault_stack_return(child, registers) else {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=<unreadable>"),
        );
        return None;
    };
    if return_address == 0 {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=0x00000000"),
        );
        return None;
    }
    let (return_owner, return_rva) =
        child_pc_owner(child, return_address).unwrap_or(("unknown", 0));
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=0x{return_address:08x} owner=\"{return_owner}\" rva=0x{return_rva:08x}",
        ),
    );
    let Some(bytes) = code_before_return(&child.address_space, return_address) else {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD NULL CALL BYTES end=0x{return_address:08x} bytes=\"<unreadable>\""),
        );
        return None;
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL CALL BYTES end=0x{return_address:08x} bytes=\"{}\"",
            diagnostic_hex_bytes(&bytes),
        ),
    );
    match classify_null_call_source(&bytes, registers, |slot| {
        let mut word = [0; 4];
        (child.address_space.read(slot, &mut word).ok()? == word.len())
            .then(|| u32::from_le_bytes(word))
    }) {
        NullCallSource::Register { name, target } => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-register register={name} target=0x{target:08x}",
                ),
            );
            None
        }
        NullCallSource::AbsoluteMemory { slot, target } => {
            let (slot_owner, slot_rva) = child_pc_owner(child, slot).unwrap_or(("unknown", 0));
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-absolute-memory slot=0x{slot:08x} slot_owner=\"{slot_owner}\" slot_rva=0x{slot_rva:08x} target={}",
                    target.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "<unreadable>".into()),
                ),
            );
            if target == Some(0) {
                log_null_slot_provenance(child, slot, target);
                Some(NullLoopSignature {
                    pid,
                    tid,
                    return_address,
                    slot,
                })
            } else {
                None
            }
        }
        NullCallSource::RegisterMemory { name, displacement, slot, target } => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-register-memory base={name} displacement=0x{:08x} slot=0x{slot:08x} target={}",
                    displacement as u32,
                    target.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "<unreadable>".into()),
                ),
            );
            None
        }
        NullCallSource::Unknown => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=unclassified",
                ),
            );
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SehRegistration { frame: u32, next: u32, handler: u32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnwindTargetRelation {
    ExitUnwind,
    CurrentHead,
    ActiveSehRegistration,
    LaterRegistration(u32),
    NotInChain,
}

impl UnwindTargetRelation {
    const fn name(self) -> &'static str {
        match self {
            Self::ExitUnwind => "exit-unwind",
            Self::CurrentHead => "current-head",
            Self::ActiveSehRegistration => "active-seh-registration",
            Self::LaterRegistration(_) => "later-registration",
            Self::NotInChain => "not-in-chain",
        }
    }
}

pub(super) fn rtl_unwind_current_target_registers(
    registers: Registers,
    provider_esp: u32,
    target_ip: u32,
    return_value: u32,
) -> Result<Registers, &'static str> {
    let mut resumed = registers;
    resumed.eip = target_ip;
    resumed.esp = provider_esp
        .checked_add(20)
        .ok_or("RtlUnwind resume ESP overflow")?;
    resumed.eax = return_value;
    Ok(resumed)
}

fn read_seh_registration(address_space: &AddressSpace, frame: u32) -> Result<SehRegistration, String> {
    if frame == 0 || frame & 3 != 0 { return Err("invalid SEH registration frame".into()); }
    let mut bytes = [0; 8];
    if address_space.read(frame, &mut bytes).map_err(|error| error.to_string())? != bytes.len() { return Err("short SEH registration read".into()); }
    let next = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    let handler = u32::from_le_bytes(bytes[4..].try_into().unwrap());
    if handler == 0 { return Err("SEH registration has zero handler".into()); }
    if next == frame { return Err("SEH registration self-loop".into()); }
    Ok(SehRegistration { frame, next, handler })
}

fn read_seh_chain_head(address_space: &AddressSpace, fs_base: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    if address_space.read(fs_base, &mut bytes).map_err(|error| error.to_string())? != bytes.len() {
        return Err("short SEH chain head read".into());
    }
    Ok(u32::from_le_bytes(bytes))
}

fn classify_unwind_target(
    child: &PendingChild,
    current_head: u32,
    target_frame: u32,
) -> Result<UnwindTargetRelation, String> {
    if target_frame == 0 {
        return Ok(UnwindTargetRelation::ExitUnwind);
    }
    if target_frame == current_head {
        return Ok(UnwindTargetRelation::CurrentHead);
    }
    if child.seh.as_ref().is_some_and(|seh| seh.registration == target_frame) {
        return Ok(UnwindTargetRelation::ActiveSehRegistration);
    }

    let mut frame = current_head;
    let mut visited = HashSet::new();
    for links in 0..MAX_SEH_CHAIN_DEPTH {
        if frame == u32::MAX {
            return Ok(UnwindTargetRelation::NotInChain);
        }
        if !visited.insert(frame) {
            return Err("SEH registration loop while classifying RtlUnwind target".into());
        }
        if frame == target_frame {
            return Ok(UnwindTargetRelation::LaterRegistration(links));
        }
        frame = read_seh_registration(&child.address_space, frame)?.next;
    }
    if frame == u32::MAX {
        Ok(UnwindTargetRelation::NotInChain)
    } else {
        Err("SEH chain exceeds RtlUnwind classification depth".into())
    }
}

fn begin_child_seh_dispatch(child: &mut PendingChild, guest: &mut GuestContext, exception: ChildException, registers: Registers) -> Result<(), String> {
    if child.seh.is_some() { return Err("nested-SEH frontier".into()); }
    let mut head = [0; 4];
    if child.address_space.read(registers.fs_base, &mut head).map_err(|error| error.to_string())? != 4 { return Err("short SEH chain head read".into()); }
    let head = u32::from_le_bytes(head);
    if head == u32::MAX { return Err("unhandled-SEH-chain frontier".into()); }
    let registration = read_seh_registration(&child.address_space, head)?;
    let scan_single_step = war3_scan_single_step(exception, registers);
    let dword_scan_single_step = war3_dword_scan_single_step(exception, registers);
    let boring_single_step = boring_war3_single_step(exception, registers, registration.handler);
    let quiet = quiet_war3_exception(exception, registers) || boring_single_step;
    if !quiet && registration.handler == WAR3_DIVIDE_EXCEPTION_HANDLER && !child.seh_handler_dumped {
        child.seh_handler_dumped = true;
        let mut bytes = [0; 128];
        let readable = child.address_space.read(registration.handler, &mut bytes).ok() == Some(bytes.len());
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SEH HANDLER DUMP pid={} tid={} handler=0x{:08x} bytes=\"{}\"",
                child.pid,
                child.tid,
                registration.handler,
                if readable {
                    diagnostic_hex_bytes(&bytes)
                } else {
                    "<unreadable>".into()
                },
            ),
        );
    }
    let debug_registers = guest
        .context
        .debug_registers()
        .map_err(|error| error.to_string())?;
    if exception.vector == Some(1) && !quiet {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD PRE-SEH DEBUG pid={} tid={} eip=0x{:08x} dr0=0x{:08x} dr1=0x{:08x} dr2=0x{:08x} dr3=0x{:08x} dr6=0x{:08x} dr7=0x{:08x} qualification_dr6={:?}",
                child.pid,
                child.tid,
                registers.eip,
                debug_registers.dr0,
                debug_registers.dr1,
                debug_registers.dr2,
                debug_registers.dr3,
                debug_registers.dr6,
                debug_registers.dr7,
                exception.debug_status,
            ),
        );
    }
    let context = wc3::seh::encode_x86_context(registers, Some(debug_registers));
    let (record, exception_code, exception_kind) = match exception.vector {
        Some(14) => {
            let linear = exception.fault_linear.ok_or("page fault linear address")?;
            let error = exception.error.ok_or("page fault error")?;
            (
                wc3::seh::encode_page_fault_exception_record(registers.eip, linear, error),
                wc3::seh::STATUS_ACCESS_VIOLATION,
                if error & 0x10 != 0 { "execute" } else if error & 2 != 0 { "write" } else { "read" },
            )
        }
        Some(1) => (
            wc3::seh::encode_single_step_exception_record(registers.eip),
            wc3::seh::STATUS_SINGLE_STEP,
            "single-step",
        ),
        Some(0) => (
            wc3::seh::encode_integer_divide_by_zero_exception_record(registers.eip),
            wc3::seh::STATUS_INTEGER_DIVIDE_BY_ZERO,
            "integer-divide-by-zero",
        ),
        _ => return Err("unsupported-exception-mapping frontier".into()),
    };
    let context_va = registers.esp.checked_sub(wc3::seh::X86_CONTEXT_BYTES as u32).ok_or("SEH context stack underflow")? & !15;
    let record_va = context_va.checked_sub(wc3::seh::EXCEPTION_RECORD_BYTES as u32).ok_or("SEH record stack underflow")?;
    let frame_esp = record_va.checked_sub(20).ok_or("SEH call stack underflow")?;
    for (address, bytes) in [(context_va, context.as_slice()), (record_va, record.as_slice())] {
        if child.address_space.write(address, bytes).map_err(|error| error.to_string())? != bytes.len() { return Err("short SEH scratch write".into()); }
    }
    let frame = [thunk32::CHILD_SEH_RETURN_ADDRESS, record_va, registration.frame, context_va, 0];
    let mut bytes = [0; 20]; for (index, value) in frame.into_iter().enumerate() { bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes()); }
    if child.address_space.write(frame_esp, &bytes).map_err(|error| error.to_string())? != bytes.len() { return Err("short SEH handler frame write".into()); }
    child.seh = Some(ChildSehDispatch { original_registers: registers, registration: registration.frame, next_registration: registration.next, handler: registration.handler, exception_record_va: record_va, context_va, preserved_fs_base: registers.fs_base, depth: 1, quiet, boring_single_step, scan_single_step, dword_scan_single_step });
    let handler_registers = wc3::seh::exception_handler_registers(
        registers,
        registration.handler,
        frame_esp,
    );
    if !quiet { logl::log(level::IMPORTANT, format_args!(
        "WC3 CHILD SEH ENTER FLAGS interrupted=0x{:08x} saved_context=0x{:08x} handler_live=0x{:08x} tf_cleared={}",
        registers.eflags,
        registers.eflags,
        handler_registers.eflags,
        u32::from(registers.eflags & wc3::seh::X86_EFLAGS_TF != 0 && handler_registers.eflags & wc3::seh::X86_EFLAGS_TF == 0),
    )); }
    guest.context.set_registers(handler_registers).map_err(|error| error.to_string())?;
    let (owner, rva) = child_pc_owner(child, registration.handler).unwrap_or(("unknown", 0));
    if !quiet { logl::log(level::IMPORTANT, format_args!("WC3 CHILD SEH DISPATCH pid={} tid={} registration=0x{:08x} next=0x{:08x} handler=0x{:08x} handler_owner={:?} handler_rva=0x{:08x} exception=0x{:08x} address=0x{:08x} kind={}", child.pid, child.tid, registration.frame, registration.next, registration.handler, owner, rva, exception_code, registers.eip, exception_kind)); }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProcSelector {
    Name(String),
    Ordinal(u16),
}

impl ProcSelector {
    fn provider_symbol(&self) -> child_loader::ProviderSymbol {
        match self {
            Self::Name(name) => child_loader::ProviderSymbol::Name(name.clone()),
            Self::Ordinal(ordinal) => child_loader::ProviderSymbol::Ordinal(*ordinal),
        }
    }
}

pub(super) fn eax_absolute_store(bytes: &[u8]) -> Option<u32> {
    if bytes.len() >= 5 && bytes[0] == 0xa3 {
        return Some(u32::from_le_bytes(bytes[1..5].try_into().unwrap()));
    }
    if bytes.len() >= 6 && bytes[0] == 0x89 && bytes[1] == 0x05 {
        return Some(u32::from_le_bytes(bytes[2..6].try_into().unwrap()));
    }
    None
}

fn log_get_proc_address_continuation(
    child: &PendingChild,
    pid: u32,
    tid: u32,
    module: &str,
    caller_return: u32,
    selector: &ProcSelector,
    result: u32,
) {
    let mut bytes = [0; 16];
    let readable = child.address_space.read(caller_return, &mut bytes).ok() == Some(bytes.len());
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD GETPROCADDRESS CALLER pid={} tid={} module={:?} selector={:?} result=0x{:08x} return=0x{:08x} after=\"{}\"",
            pid,
            tid,
            module,
            selector,
            result,
            caller_return,
            if readable {
                diagnostic_hex_bytes(&bytes)
            } else {
                "<unreadable>".into()
            },
        ),
    );
    if !readable {
        return;
    }
    if let Some(destination) = eax_absolute_store(&bytes) {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD GETPROCADDRESS STORE pid={} tid={} module={:?} selector={:?} result=0x{:08x} return=0x{:08x} destination=0x{:08x}",
                pid, tid, module, selector, result, caller_return, destination,
            ),
        );
    }
}

fn install_child_provider_imports(
    child: &mut PendingChild,
    process: &mut XpProcess,
    imports: Vec<child_loader::ProviderImport>,
) -> Result<Vec<u32>, String> {
    let (addresses, old_bytes, new_bytes, updated_from, updated) = process
        .append_provider_imports(imports)
        .map_err(str::to_owned)?;
    child.provider_thunk_bytes = new_bytes;
    if new_bytes > old_bytes {
        child
            .address_space
            .map(
                thunk32::THUNK_BASE + old_bytes as u32,
                new_bytes - old_bytes,
                Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
            )
            .map_err(|error| error.to_string())?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD PROVIDER THUNK GROW pid={} old_bytes={} new_bytes={}",
                child.pid, old_bytes, new_bytes
            ),
        );
    }
    let existing_update = old_bytes.saturating_sub(updated_from).min(updated.len());
    if existing_update != 0 {
        child
            .address_space
            .write(
                thunk32::THUNK_BASE + updated_from as u32,
                &updated[..existing_update],
            )
            .map_err(|error| error.to_string())?;
    }
    if existing_update < updated.len() {
        child
            .address_space
            .write(
                thunk32::THUNK_BASE + old_bytes as u32,
                &updated[existing_update..],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(addresses)
}

pub(super) async fn run_loop(
    address_space: &AddressSpace,
    mut memory: X86Memory<'_>,
    mut session: &mut Wc3Session,
    mut contexts: &mut Vec<GuestContext>,
    pending_child: &mut Option<PendingChild>,
    thread_calls: &mut HashMap<(u32, u32), u32>,
    default_proc_messages: &mut HashSet<u32>,
    mut frames: &mut HashMap<u32, Frame>,
    window_rgba: &mut HashMap<u32, Vec<u8>>,
    blit_checkpoint_done: &mut bool,
    mut wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
    mut previous_wait_timeout: &mut Option<(ThreadKey, u32, u32)>,
    child_get_command_line_logged: &mut bool,
    active_message_box: &mut Option<ActiveMessageBox>,
    mut active: usize,
) -> Result<(), String> {
    loop {
        if let Some(modal) = active_message_box.as_mut() {
            if modal.request.caller.pid != LAUNCHER_PID || modal.buttons.is_empty() {
                return Err("invalid active MessageBoxA modal state".into());
            }
            while modal
                .frame
                .take_pointer_event()
                .map_err(|error| format!("poll MessageBoxA pointer event: {error:?}"))?
                .is_some()
            {}
            trueos::vsys::poll_once();
            tokio::task::yield_now().await;
            trueos::vsys::sleep_ms(8);
            continue;
        }
        let exit = if contexts[active].started {
            contexts[active].context.resume().await
        } else {
            contexts[active].started = true;
            contexts[active].context.run().await
        }
        .map_err(|error| error.to_string())?;
        let active_key = contexts
            .get(active)
            .ok_or_else(|| "active guest context missing".to_owned())?
            .key();
        match exit.kind {
            // A transient VMCS always starts with VMLAUNCH.  Its preemption
            // timer is therefore a Blueprint scheduling boundary, not an x86
            // program stop: Context::resume() restores the logical context
            // into a fresh VMCS on whichever Tokio carrier runs next.
            ExitKind::Other if exit.detail == 52 => {
                let (preemptions, same_page) = {
                    let context = contexts
                        .get_mut(active)
                        .ok_or_else(|| "active guest context missing".to_owned())?;
                    context.preemption_count = context
                        .preemption_count
                        .checked_add(1)
                        .ok_or_else(|| "preemption count overflow".to_owned())?;
                    let page = exit.registers.eip & !0xfff;
                    if context.last_preemption_page == page {
                        context.same_page_preemptions =
                            context.same_page_preemptions.saturating_add(1);
                    } else {
                        context.last_preemption_page = page;
                        context.same_page_preemptions = 1;
                    }
                    (context.preemption_count, context.same_page_preemptions)
                };
                if active_key.pid != LAUNCHER_PID
                    && should_log_execution_sample(preemptions)
                    && pending_child
                        .as_ref()
                        .is_some_and(|child| {
                            child.execution == ChildExecutionState::ImageEntryRunning
                                && !(child.scan_progress.is_some()
                                    && (WAR3_SCAN_STEP_START..=WAR3_SCAN_STEP_END)
                                        .contains(&exit.registers.eip))
                        })
                {
                    let child = pending_child.as_ref().unwrap();
                    let (owner, rva) = child_pc_owner(child, exit.registers.eip)
                        .unwrap_or(("unknown", 0));
                    let mut code = [0u8; 16];
                    let code_len = child.address_space.read(exit.registers.eip, &mut code).unwrap_or(0);
                    let code = code[..code_len]
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXEC SAMPLE pid={} tid={} during=\"War3.exe:ENTRY\" preemptions={} owner={:?} rva=0x{:08x} eip=0x{:08x} esp=0x{:08x} ebp=0x{:08x} eax=0x{:08x} same_page={} code={}",
                            active_key.pid, active_key.tid, preemptions, owner, rva,
                            exit.registers.eip, exit.registers.esp, exit.registers.ebp,
                            exit.registers.eax, same_page, code,
                        ),
                    );
                    let scan_index = child_read_u32(child, WAR3_SCAN_INDEX);
                    let scan_source = child_read_u32(child, WAR3_SCAN_SOURCE);
                    let scan_bound = child_read_u32(child, WAR3_SCAN_BOUND);
                    let scan_count = child_read_u32(child, WAR3_SCAN_COUNT);
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD HOTLOOP pid={} tid={} preemptions={} eip=0x{:08x} index={} source={} bound={} count={} eflags=0x{:08x}",
                            active_key.pid,
                            active_key.tid,
                            preemptions,
                            exit.registers.eip,
                            scan_index
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_source
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_bound
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_count
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            exit.registers.eflags,
                        ),
                    );
                    if same_page == 1024 {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD EXEC HOTPAGE pid={} tid={} owner={:?} page=0x{:08x} eip=0x{:08x} samples={}",
                                active_key.pid, active_key.tid, owner,
                                exit.registers.eip & !0xfff, exit.registers.eip, same_page,
                            ),
                        );
                    }
                }
                expire_runtime_waits(
                    &mut session,
                    &mut contexts,
                    &mut wait_deadlines,
                    &mut previous_wait_timeout,
                )?;
                if !session.blocked.contains_key(&active_key)
                    && context_index(&contexts, active_key).is_some()
                {
                    session.enqueue(active_key);
                }
                if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                    active = next;
                }
                tokio::task::yield_now().await;
                continue;
            }
            ExitKind::VmCall => {
                let active_pid = active_key.pid;
                let active_tid = active_key.tid;
                if active_pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_mut()
                        .filter(|child| child.pid == active_pid && child.tid == active_tid)
                        .ok_or_else(|| "active child address space missing".to_owned())?;
                    if exit.registers.eip == thunk32::CHILD_UEF_RETURN_AFTER_VMCALL {
                        let pending = child.unhandled_filter_call.take().ok_or("UEF return without pending filter call")?;
                        let result = session.process(active_pid).ok_or_else(|| "child process missing".to_owned())?.xp.complete_unhandled_exception_filter(Some(exit.registers.eax)).map_err(str::to_owned)?;
                        let mut registers = exit.registers; registers.eip = pending.provider_resume_eip; registers.esp = pending.provider_esp; registers.eax = result;
                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        logl::log(level::IMPORTANT, format_args!("WC3 CHILD UEF FILTER RETURN pid={} tid={} filter=0x{:08x} filter_result=0x{:08x} uef_result=0x{:08x}", active_pid, active_tid, pending.filter, exit.registers.eax, result));
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_SEH_RETURN_AFTER_VMCALL {
                        let seh = child.seh.take().ok_or("SEH return without pending dispatch")?;
                        if !seh.quiet { logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD SEH RETURN pid={} tid={} registration=0x{:08x} handler=0x{:08x} disposition={}",
                            active_pid, active_tid, seh.registration, seh.handler, exit.registers.eax,
                        )); }
                        if exit.registers.eax != wc3::seh::DISPOSITION_CONTINUE_EXECUTION {
                            return Err(format!("WC3 CHILD SEH FRONTIER reason=unsupported-disposition value={}", exit.registers.eax));
                        }
                        let mut bytes = [0; wc3::seh::X86_CONTEXT_BYTES];
                        if child.address_space.read(seh.context_va, &mut bytes).map_err(|error| error.to_string())? != bytes.len() { return Err("short SEH context readback".into()); }
                        let mut restored = wc3::seh::decode_x86_context(&bytes, seh.preserved_fs_base).map_err(str::to_owned)?;
                        let mut restored_debug = wc3::seh::decode_x86_debug_registers(&bytes)
                            .map_err(str::to_owned)?;
                        if restored.eip == TABLE_CHECKPOINT_FROM_EIP
                            && child_read_u32(child, WAR3_DWORD_SCAN_INDEX) == Some(0)
                        {
                            let restored_checkpoint = try_restore_table_checkpoint(
                                child,
                                &mut contexts[active].context,
                                &mut restored,
                                &mut restored_debug,
                            )
                            .await?;
                            if !restored_checkpoint {
                                begin_table_checkpoint_capture(
                                    child,
                                    &contexts[active].context,
                                    restored,
                                    restored_debug,
                                    &session,
                                )?;
                            }
                        }
                        if seh.boring_single_step {
                            let control_changed = restored.eip != seh.original_registers.eip
                                || restored.esp != seh.original_registers.esp
                                || ((restored.eflags ^ seh.original_registers.eflags)
                                    & wc3::seh::X86_EFLAGS_TF) != 0;
                            if control_changed {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SINGLESTEP TRANSITION old_eip=0x{:08x} new_eip=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x} old_eflags=0x{:08x} new_eflags=0x{:08x} dr6=0x{:08x} dr7=0x{:08x}",
                                        seh.original_registers.eip,
                                        restored.eip,
                                        seh.original_registers.esp,
                                        restored.esp,
                                        seh.original_registers.eflags,
                                        restored.eflags,
                                        restored_debug.dr6,
                                        restored_debug.dr7,
                                    ),
                                );
                            }
                            child.single_step_count = child.single_step_count.saturating_add(1);
                            if child.single_step_count == 1 || child.single_step_count % 0x1000 == 0 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SINGLESTEP HEARTBEAT count={} eip=0x{:08x} tf={} dr6=0x{:08x} dr7=0x{:08x}",
                                        child.single_step_count,
                                        restored.eip,
                                        u32::from(restored.eflags & wc3::seh::X86_EFLAGS_TF != 0),
                                        restored_debug.dr6,
                                        restored_debug.dr7,
                                    ),
                                );
                            }
                        }
                        if !seh.quiet
                            && matches!(seh.original_registers.eip, 0x0045_af51 | 0x0045_af54 | 0x0045_af5a)
                        {
                            let get = |offset: usize| {
                                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH DEBUG RETURN eip=0x{:08x} dr0=0x{:08x} dr1=0x{:08x} dr2=0x{:08x} dr3=0x{:08x} dr6=0x{:08x} dr7=0x{:08x}",
                                    seh.original_registers.eip,
                                    get(0x04),
                                    get(0x08),
                                    get(0x0c),
                                    get(0x10),
                                    get(0x14),
                                    get(0x18),
                                ),
                            );
                        }
                        if seh.scan_single_step {
                            log_war3_scan_progress(
                                child,
                                seh.original_registers.eip,
                                restored,
                                restored_debug,
                            );
                        }
                        if seh.dword_scan_single_step {
                            observe_war3_dword_scan(child, seh.original_registers.eip);
                        }
                        if !seh.quiet {
                            let raw_ecx = u32::from_le_bytes(
                                bytes[wc3::seh::ECX_OFFSET..wc3::seh::ECX_OFFSET + 4]
                                    .try_into()
                                    .unwrap(),
                            );
                            logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD SEH CONTEXT RETURN pid={} tid={} context=0x{:08x} old_ecx=0x{:08x} saved_ecx=0x{:08x} restored_ecx=0x{:08x}",
                            active_pid, active_tid, seh.context_va, seh.original_registers.ecx, raw_ecx, restored.ecx,
                            ));
                        }
                        if restored.eip == TABLE_CHECKPOINT_TO_EIP
                            && child_read_u32(child, WAR3_DWORD_SCAN_INDEX)
                                == Some(WAR3_TABLE_FILL_BOUND)
                            && child.table_checkpoint_capture.is_some()
                        {
                            finish_table_checkpoint_capture(
                                child,
                                &contexts[active].context,
                                restored,
                                restored_debug,
                                &session,
                            )
                            .await?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD TABLE CHECKPOINT CONTINUE reason=artifact-verified"
                                ),
                            );
                        }
                        contexts[active].context.set_registers(restored).map_err(|error| error.to_string())?;
                        contexts[active]
                            .context
                            .set_debug_registers(restored_debug)
                            .map_err(|error| error.to_string())?;
                        if !seh.quiet { logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD SEH CONTINUE pid={} tid={} old_eip=0x{:08x} new_eip=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x}",
                            active_pid, active_tid, seh.original_registers.eip, restored.eip, seh.original_registers.esp, restored.esp,
                        )); }
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_DLL_RETURN_AFTER_VMCALL {
                        let native_index = match child.execution {
                            ChildExecutionState::DllInitRunning { native_index } => native_index,
                            ChildExecutionState::Loader => {
                                return Err("child DLL return while execution=loader".into());
                            }
                            ChildExecutionState::DllInitReady { .. } => {
                                return Err("child DLL return while DLL init is not running".into());
                            }
                            ChildExecutionState::ImageEntryReady
                            | ChildExecutionState::ImageEntryRunning => {
                                return Err("child DLL return while image entry is active".into());
                            }
                        };
                        let module = child
                            .native_modules
                            .get(native_index)
                            .ok_or_else(|| "child DLL return native index".to_owned())?;
                        let module_name = module.stored.clone();
                        let success = exit.registers.eax != 0;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL INIT RETURN pid={} tid={} module=\"{}\" eax=0x{:08x} success={}",
                                active_pid,
                                active_tid,
                                module_name,
                                exit.registers.eax,
                                success as u8
                            ),
                        );
                        if !success {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT FAILED pid={} module=\"{}\" reason=DLL_PROCESS_ATTACH-returned-FALSE",
                                    active_pid, module_name
                                ),
                            );
                            return Ok(());
                        }
                        child.native_modules[native_index].initialized = true;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD NATIVE MODULE INITIALIZED pid={} module=\"{}\" base=0x{:08x} initialized=1",
                                active_pid,
                                module_name,
                                child.native_modules[native_index].image.image_base
                            ),
                        );
                        let next_index = child
                            .native_modules
                            .iter()
                            .position(|module| !module.initialized);
                        let Some(next_index) = next_index else {
                            let initialized = child
                                .native_modules
                                .iter()
                                .filter(|module| module.initialized)
                                .count();
                            if initialized != child.native_modules.len() {
                                return Err("child loader completion count mismatch".into());
                            }
                            child.execution = ChildExecutionState::ImageEntryReady;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADER COMPLETE pid={} tid={} modules={} initialized={}",
                                    child.pid, child.tid, child.native_modules.len(), initialized
                                ),
                            );
                            log_child_entry_null_slot_check(child);
                            let (entry, frame_esp) = arm_existing_child_image_entry(
                                child,
                                &mut contexts[active],
                                exit.registers,
                            )?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD IMAGE ENTRY RESUME pid={} tid={} image=\"War3.exe\" base=0x{:08x} entry=0x{:08x} esp=0x{:08x} return=0x{:08x} caller_headroom={} caller_bytes={} stack_top=0x{:08x} context_reused=1 teb_preserved=1 xstate_preserved=1",
                                    active_pid,
                                    active_tid,
                                    child.image.image_base,
                                    entry,
                                    frame_esp,
                                    thunk32::CHILD_IMAGE_RETURN_ADDRESS,
                                    CHILD_IMAGE_ENTRY_HEADROOM,
                                    CHILD_IMAGE_ENTRY_CALLER_BYTES,
                                    STACK_TOP,
                                ),
                            );
                            continue;
                        };
                        let previous_name = module_name;
                        child.execution = ChildExecutionState::DllInitReady { native_index: next_index };
                        let (next_name, next_entry, frame_esp) = arm_existing_child_dll_init(
                            child,
                            &mut contexts[active],
                            next_index,
                            exit.registers,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL REARM pid={} tid={} previous=\"{}\" next=\"{}\" context_reused=1 xstate_preserved=1 teb_preserved=1 entry=0x{:08x} esp=0x{:08x}",
                                active_pid, active_tid, previous_name, next_name, next_entry, frame_esp
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL FRAME pid={} tid={} module=\"{}\" return=0x{:08x} hinst=0x{:08x} reason=1 reserved=0x{:08x}",
                                active_pid,
                                active_tid,
                                next_name,
                                thunk32::CHILD_DLL_RETURN_ADDRESS,
                                child.native_modules[next_index].image.image_base,
                                child.static_load_reserved,
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL INIT RESUME pid={} tid={} module=\"{}\" state=dll-init-running context_started=1",
                                active_pid, active_tid, next_name
                            ),
                        );
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_IMAGE_RETURN_AFTER_VMCALL {
                        if child.execution != ChildExecutionState::ImageEntryRunning {
                            return Err("child image return outside image-entry-running state".into());
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD IMAGE RETURN pid={} tid={} eax=0x{:08x}",
                                active_pid, active_tid, exit.registers.eax
                            ),
                        );
                        return Ok(());
                    }
                    if exit.registers.eip == thunk32::CHILD_THREAD_EXIT_AFTER_VMCALL {
                        let scope = child_execution_scope(child).map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CONTROL FRONTIER pid={} tid={} during=\"{}\" kind=thread-exit",
                                active_pid, active_tid, scope
                            ),
                        );
                        return Ok(());
                    }
                    if exit.registers.eip == thunk32::CHILD_CIPOW_SPILL_AFTER_VMCALL {
                        let pending = child.cipow.take().ok_or_else(|| "CIPOW spill without pending state".to_owned())?;
                        if exit.registers.esp != pending.provider_esp { return Err("CIPOW spill ESP mismatch".into()); }
                        let exponent = read_guest_words(&X86Memory(&child.address_space), thunk32::CHILD_CIPOW_EXPONENT_ADDRESS, 2)?;
                        let base = read_guest_words(&X86Memory(&child.address_space), thunk32::CHILD_CIPOW_BASE_ADDRESS, 2)?;
                        let exponent = f64::from_bits((exponent[0] as u64) | ((exponent[1] as u64) << 32));
                        let base = f64::from_bits((base[0] as u64) | ((base[1] as u64) << 32));
                        if !child.cipow_diagnostic_logged {
                            child.cipow_diagnostic_logged = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT CIPOW base={} exponent={}",
                                    base, exponent
                                ),
                            );
                        }
                        if !base.is_finite() || !exponent.is_finite() || base <= 0.0 { return Ok(()); }
                        let result = base.powf(exponent);
                        if !result.is_finite() { return Ok(()); }
                        child.address_space.write(thunk32::CHILD_CIPOW_RESULT_ADDRESS, &result.to_bits().to_le_bytes()).map_err(|error| error.to_string())?;
                        let mut registers = exit.registers;
                        registers.eip = thunk32::CHILD_CIPOW_RESTORE_ADDRESS;
                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL {
                        if let Some(initterm) = child.initterm.as_ref() {
                            if exit.registers.esp != initterm.provider_esp {
                                return Err(format!(
                                    "child _initterm callback ESP mismatch expected=0x{:08x} actual=0x{:08x}",
                                    initterm.provider_esp, exit.registers.esp
                                ));
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT INITTERM RETURN pid={} tid={} completed={} eax=0x{:08x}",
                                    active_pid, active_tid, initterm.callbacks_invoked, exit.registers.eax
                                ),
                            );
                            match advance_child_initterm(child, &mut contexts[active])? {
                                InittermAdvance::CallbackScheduled | InittermAdvance::Complete => continue,
                            }
                        }
                        let scope = child_execution_scope(child).map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CONTROL FRONTIER pid={} tid={} during=\"{}\" kind=callback-return",
                                active_pid, active_tid, scope
                            ),
                        );
                        return Ok(());
                    }
                    let running_scope = child_execution_scope(child).map_err(str::to_owned)?;
                    let running_module_name = running_scope.clone();
                    let provider_id = exit.registers.eax;
                    let provider = session
                        .process(active_pid)
                        .and_then(|process| process.xp.provider_import(provider_id))
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "unknown child provider trap pid={} tid={} id={}",
                                active_pid, active_tid, provider_id
                            )
                        })?;
                    let mut caller_ret = [0; 4];
                    child
                        .address_space
                        .read(exit.registers.esp, &mut caller_ret)
                        .map_err(|error| error.to_string())?;
                    let symbol = match &provider.symbol {
                        child_loader::ProviderSymbol::Name(name) => format!("symbol=\"{}\"", name),
                        child_loader::ProviderSymbol::Ordinal(ordinal) => {
                            format!("ordinal={}", ordinal)
                        }
                    };
                    let is_heap_create = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapCreate"
                    );
                    if is_heap_create {
                        let mut child_memory = X86Memory(&child.address_space);
                        let heap = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .create_win_heap(exit.registers.esp, &mut child_memory)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP CREATE pid={} tid={} during=\"{}\" provider_id={} options=0x{:08x} initial_size=0x{:08x} maximum_size=0x{:08x} handle=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap.options,
                                heap.initial_size,
                                heap.maximum_size,
                                heap.handle,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = heap.handle;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP CREATE RESULT pid={} tid={} handle=0x{:08x} resume_eip=0x{:08x} esp=0x{:08x} cleanup=12-by-thunk",
                                active_pid,
                                active_tid,
                                heap.handle,
                                registers.eip,
                                registers.esp,
                            ),
                        );
                        continue;
                    }
                    let is_heap_alloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapAlloc"
                    );
                    if is_heap_alloc {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let heap = frame[1];
                        let flags = frame[2];
                        let bytes = frame[3];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP ALLOC CALL pid={} tid={} during=\"{}\" provider_id={} heap=0x{:08x} flags=0x{:08x} bytes={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap,
                                flags,
                                bytes,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let allocation = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .alloc_win_heap(
                                exit.registers.esp,
                                &X86Memory(&child.address_space),
                            )
                            .map_err(str::to_owned)?;
                        let pointer = if let Some(allocation) = allocation {
                            let mapped_end =
                                ensure_child_win_heap_mapped(child, allocation.end)?;
                            if allocation.flags & 0x8 != 0 {
                                let zeroes = vec![0; allocation.requested.max(1) as usize];
                                let written = child
                                    .address_space
                                    .write(allocation.pointer, &zeroes)
                                    .map_err(|error| error.to_string())?;
                                if written != zeroes.len() {
                                    return Err("short child HeapAlloc zero write".into());
                                }
                            }
                            if allocation.pointer < CHILD_WIN_HEAP_BASE
                                || allocation.pointer >= CHILD_WIN_HEAP_LIMIT
                                || allocation.pointer % 8 != 0
                                || allocation.end > mapped_end
                            {
                                return Err("child HeapAlloc pointer verification failed".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP ALLOC RESULT pid={} tid={} heap=0x{:08x} flags=0x{:08x} requested={} pointer=0x{:08x} end=0x{:08x} mapped_end=0x{:08x} zeroed={} cleanup=12-by-thunk",
                                    active_pid,
                                    active_tid,
                                    allocation.heap,
                                    allocation.flags,
                                    allocation.requested,
                                    allocation.pointer,
                                    allocation.end,
                                    mapped_end,
                                    (allocation.flags & 0x8 != 0) as u8,
                                ),
                            );
                            allocation.pointer
                        } else {
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = pointer;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_heap_free = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapFree"
                    );
                    if is_heap_free {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let heap = frame[1];
                        let flags = frame[2];
                        let pointer = frame[3];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP FREE CALL pid={} tid={} during=\"{}\" provider_id={} heap=0x{:08x} flags=0x{:08x} pointer=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap,
                                flags,
                                pointer,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if flags != 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE FRONTIER pid={} tid={} reason=unsupported-flags flags=0x{:08x}",
                                    active_pid, active_tid, flags,
                                ),
                            );
                            return Ok(());
                        }
                        let freed = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .free_win_heap(exit.registers.esp, &X86Memory(&child.address_space))
                            .map_err(str::to_owned)?;
                        let result = if let Some(allocation) = freed {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE RESULT pid={} tid={} heap=0x{:08x} pointer=0x{:08x} requested={} end=0x{:08x} freed=1 cleanup=12-by-thunk",
                                    active_pid,
                                    active_tid,
                                    allocation.heap,
                                    allocation.pointer,
                                    allocation.requested,
                                    allocation.end,
                                ),
                            );
                            1
                        } else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE RESULT pid={} tid={} heap=0x{:08x} pointer=0x{:08x} freed=0 cleanup=12-by-thunk",
                                    active_pid, active_tid, heap, pointer,
                                ),
                            );
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_command_line_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetCommandLineA"
                    );
                    if is_get_command_line_a {
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("GetCommandLineA child provider did not return".into());
                        };
                        if result != PROCESS_DATA_VA {
                            return Err("child command-line pointer mismatch".into());
                        }
                        let mut bytes = [0; CHILD_COMMAND_LINE.len()];
                        child
                            .address_space
                            .read(result, &mut bytes)
                            .map_err(|error| error.to_string())?;
                        if bytes != CHILD_COMMAND_LINE {
                            return Err("child command-line backing mismatch".into());
                        }
                        let command_line =
                            String::from_utf8_lossy(&bytes[..bytes.len().saturating_sub(1)]);
                        if !*child_get_command_line_logged {
                            *child_get_command_line_logged = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETCOMMANDLINEA RESULT pid={} tid={} pointer=0x{:08x} value={:?} process_private=1 stack_cleanup=none",
                                    active_pid, active_tid, result, command_line,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_environment_strings_w = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetEnvironmentStringsW"
                    );
                    if is_get_environment_strings_w {
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("GetEnvironmentStringsW child provider did not return".into());
                        };
                        if result != ENVIRONMENT_BLOCK_VA {
                            return Err("child environment pointer mismatch".into());
                        }
                        let mut terminator = [0u8; 4];
                        child
                            .address_space
                            .read(result, &mut terminator)
                            .map_err(|error| error.to_string())?;
                        if terminator != [0, 0, 0, 0] {
                            return Err("child wide environment block is not double-NUL terminated".into());
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETENVIRONMENTSTRINGSW RESULT pid={} tid={} pointer=0x{:08x} environment=empty encoding=utf16le process_private=1 stack_cleanup=none",
                                active_pid, active_tid, result,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_wide_char_to_multi_byte = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "WideCharToMultiByte"
                    );
                    if is_wide_char_to_multi_byte {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            9,
                        )?;
                        let code_page = frame[1];
                        let flags = frame[2];
                        let source = frame[3];
                        let count = frame[4];
                        let output = frame[5];
                        let capacity = frame[6];
                        let default_char = frame[7];
                        let used_default_char = frame[8];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD WIDECHARTOMULTIBYTE CALL pid={} tid={} during=\"{}\" provider_id={} code_page={} flags=0x{:08x} source=0x{:08x} count={} output=0x{:08x} capacity={} default_char=0x{:08x} used_default_char=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                code_page,
                                flags,
                                source,
                                count as i32,
                                output,
                                capacity,
                                default_char,
                                used_default_char,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if code_page != 0 && code_page != XP_ANSI_CODE_PAGE {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE FRONTIER pid={} tid={} reason=unsupported-code-page code_page={}",
                                    active_pid, active_tid, code_page,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("WideCharToMultiByte child provider did not return".into());
                        };
                        if output == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE RESULT pid={} tid={} mode=size-query eax={} cleanup=32-by-thunk",
                                    active_pid, active_tid, result,
                                ),
                            );
                        } else {
                            let byte_count = usize::try_from(result)
                                .map_err(|_| "WideCharToMultiByte result too large".to_owned())?;
                            let mut bytes = vec![0; byte_count];
                            let read = child
                                .address_space
                                .read(output, &mut bytes)
                                .map_err(|error| error.to_string())?;
                            if read != bytes.len() {
                                return Err("short WideCharToMultiByte output read".into());
                            }
                            let mut preview = bytes
                                .iter()
                                .take(64)
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            if bytes.len() > 64 {
                                preview.push_str(" ...");
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE RESULT pid={} tid={} mode=convert eax={} output=0x{:08x} bytes=\"{}\" cleanup=32-by-thunk",
                                    active_pid, active_tid, result, output, preview,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_version_ex_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetVersionExA"
                    );
                    if is_get_version_ex_a {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let info = frame[1];
                        let size = read_guest_words(&X86Memory(&child.address_space), info, 1)?[0];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSIONEXA CALL pid={} tid={} during=\"{}\" provider_id={} info=0x{:08x} size=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                info,
                                size,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if size != 0x94 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETVERSIONEXA FRONTIER pid={} tid={} reason=unsupported-structure-size size=0x{:08x}",
                                    active_pid, active_tid, size,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("GetVersionExA child provider did not return".into());
                        };
                        let version = read_guest_words(&X86Memory(&child.address_space), info, 5)?;
                        if result != 1 || version != [0x94, 5, 1, 2600, 2] {
                            return Err("GetVersionExA child result verification failed".into());
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSIONEXA RESULT pid={} tid={} eax={} major={} minor={} build={} platform={} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                result,
                                version[1],
                                version[2],
                                version[3],
                                version[4],
                            ),
                        );
                        continue;
                    }
                    let is_get_version = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetVersion"
                    );
                    if is_get_version {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"GetVersion\" esp=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("GetVersion child provider did not return".into());
                        };
                        if value != 0x0a28_0105 {
                            return Err(format!(
                                "GetVersion result mismatch expected=0x0a280105 actual=0x{value:08x}"
                            ));
                        }
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSION RESULT pid={} tid={} eax=0x{:08x} stack_cleanup=none",
                                active_pid, active_tid, value
                            ),
                        );
                        continue;
                    }
                    let is_initialize_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "InitializeCriticalSection"
                    );
                    if is_initialize_critical_section {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut critical_section = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut critical_section)
                            .map_err(|error| error.to_string())?;
                        let critical_section = u32::from_le_bytes(critical_section);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"InitializeCriticalSection\" esp=0x{:08x} critical_section=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                critical_section
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err(
                                "InitializeCriticalSection child provider did not return".into()
                            );
                        };
                        let mut initialized = [0; 0x18];
                        child
                            .address_space
                            .read(critical_section, &mut initialized)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(
                            initialized[4..8]
                                .try_into()
                                .map_err(|_| "critical-section lock count")?,
                        );
                        let recursion = u32::from_le_bytes(
                            initialized[8..12]
                                .try_into()
                                .map_err(|_| "critical-section recursion")?,
                        );
                        let owner = u32::from_le_bytes(
                            initialized[12..16]
                                .try_into()
                                .map_err(|_| "critical-section owner")?,
                        );
                        if lock_count != u32::MAX || recursion != 0 || owner != 0 {
                            return Err(
                                "child critical-section initialization verification failed".into(),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRITICAL SECTION INIT pid={} address=0x{:08x} lock_count=0xffffffff recursion=0 owner=0 caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                                active_pid,
                                critical_section,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip,
                                value
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_enter_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "EnterCriticalSection"
                    );
                    if is_enter_critical_section {
                        let argument = exit.registers.esp.checked_add(4).ok_or_else(|| {
                            "child provider argument address overflow".to_owned()
                        })?;
                        let mut critical_section = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut critical_section)
                            .map_err(|error| error.to_string())?;
                        let critical_section = u32::from_le_bytes(critical_section);
                        let known = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .has_critical_section(critical_section);
                        if !known {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRITICAL SECTION FRONTIER pid={} tid={} operation=enter reason=unknown-critical-section address=0x{:08x}",
                                    active_pid, active_tid, critical_section
                                ),
                            );
                            return Ok(());
                        }
                        let mut before = [0; 16];
                        child
                            .address_space
                            .read(critical_section, &mut before)
                            .map_err(|error| error.to_string())?;
                        let lock_count_before = u32::from_le_bytes(before[4..8].try_into().unwrap());
                        let recursion_before = u32::from_le_bytes(before[8..12].try_into().unwrap());
                        let owner_before = u32::from_le_bytes(before[12..16].try_into().unwrap());
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"EnterCriticalSection\" esp=0x{:08x} critical_section=0x{:08x} caller_ret=0x{:08x} lock_count_before=0x{:08x} recursion_before={} owner_before={}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                critical_section,
                                u32::from_le_bytes(caller_ret),
                                lock_count_before,
                                recursion_before,
                                owner_before,
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("EnterCriticalSection child provider did not return".into());
                        };
                        let mut after = [0; 16];
                        child
                            .address_space
                            .read(critical_section, &mut after)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(after[4..8].try_into().unwrap());
                        let recursion = u32::from_le_bytes(after[8..12].try_into().unwrap());
                        let owner = u32::from_le_bytes(after[12..16].try_into().unwrap());
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRITICAL SECTION ENTER pid={} tid={} address=0x{:08x} lock_count=0x{:08x} recursion={} owner={} caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                                active_pid,
                                active_tid,
                                critical_section,
                                lock_count,
                                recursion,
                                owner,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip,
                                value,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_leave_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "LeaveCriticalSection"
                    );
                    if is_leave_critical_section {
                        let argument = exit.registers.esp.checked_add(4).ok_or_else(|| {
                            "child provider argument address overflow".to_owned()
                        })?;
                        let mut address = [0; 4];
                        child.address_space.read(argument, &mut address)
                            .map_err(|error| error.to_string())?;
                        let address = u32::from_le_bytes(address);
                        if !session.process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp.has_critical_section(address)
                        {
                            logl::log(level::IMPORTANT, format_args!(
                                "WC3 CHILD CRITICAL SECTION FRONTIER pid={} tid={} operation=leave reason=unknown-critical-section address=0x{:08x}",
                                active_pid, active_tid, address
                            ));
                            return Ok(());
                        }
                        let mut before = [0; 16];
                        child.address_space.read(address, &mut before)
                            .map_err(|error| error.to_string())?;
                        let lock_before = u32::from_le_bytes(before[4..8].try_into().unwrap());
                        let recursion_before = u32::from_le_bytes(before[8..12].try_into().unwrap());
                        let owner_before = u32::from_le_bytes(before[12..16].try_into().unwrap());
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"LeaveCriticalSection\" esp=0x{:08x} critical_section=0x{:08x} caller_ret=0x{:08x} lock_count_before=0x{:08x} recursion_before={} owner_before={}",
                            active_pid, active_tid, running_module_name, provider_id, provider.module,
                            exit.registers.esp, address, u32::from_le_bytes(caller_ret), lock_before,
                            recursion_before, owner_before
                        ));
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session.process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?.xp
                            .dispatch_provider_for_process(active_pid, active_tid, provider_id,
                                exit.registers.esp, &mut child_memory).map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("LeaveCriticalSection child provider did not return".into());
                        };
                        let mut after = [0; 16];
                        child.address_space.read(address, &mut after)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(after[4..8].try_into().unwrap());
                        let recursion = u32::from_le_bytes(after[8..12].try_into().unwrap());
                        let owner = u32::from_le_bytes(after[12..16].try_into().unwrap());
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD CRITICAL SECTION LEAVE pid={} tid={} address=0x{:08x} lock_count=0x{:08x} recursion={} owner={} caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                            active_pid, active_tid, address, lock_count, recursion, owner,
                            u32::from_le_bytes(caller_ret), exit.registers.eip, value
                        ));
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active].context.set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_set_last_error = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "SetLastError"
                    );
                    if is_set_last_error {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut value = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut value)
                            .map_err(|error| error.to_string())?;
                        let value = u32::from_le_bytes(value);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"SetLastError\" esp=0x{:08x} value=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                value
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("SetLastError child provider did not return".into());
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LAST ERROR SET pid={} tid={} value=0x{:08x}",
                                active_pid, active_tid, value
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_cipow = matches!(&provider.symbol, child_loader::ProviderSymbol::Name(name) if provider.module.eq_ignore_ascii_case("MSVCRT.dll") && name == "_CIpow");
                    if is_cipow {
                        if child.cipow.is_some() { return Err("nested CIPOW".into()); }
                        child.cipow = Some(ChildCiPow { provider_esp: exit.registers.esp });
                        let mut registers = exit.registers;
                        registers.eip = thunk32::CHILD_CIPOW_SPILL_ADDRESS;
                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_unhandled_filter = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "SetUnhandledExceptionFilter"
                    );
                    if is_unhandled_filter {
                        let filter = read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 2)?[1];
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session.process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?.xp
                            .dispatch_provider_for_process(active_pid, active_tid, provider_id, exit.registers.esp, &mut child_memory)
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(previous) = action else { return Err("SetUnhandledExceptionFilter child provider did not return".into()); };
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD UNHANDLED FILTER SET pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" filter=0x{:08x} previous=0x{:08x} caller_ret=0x{:08x} resume_eip=0x{:08x}",
                            active_pid, active_tid, running_module_name, filter, previous,
                            u32::from_le_bytes(caller_ret), exit.registers.eip
                        ));
                        let mut registers = exit.registers;
                        registers.eax = previous;
                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_malloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "malloc"
                    );
                    if is_crt_malloc {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut size = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut size)
                            .map_err(|error| error.to_string())?;
                        let size = u32::from_le_bytes(size);
                        if size == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"malloc\" reason=zero-size-unobserved",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    provider_id,
                                    provider.module,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(pointer) = action else {
                            return Err("malloc child provider did not return".into());
                        };
                        if pointer == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT MALLOC pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" size={} pointer=0x00000000 result=out-of-memory",
                                    active_pid, active_tid, running_module_name, size,
                                ),
                            );
                        } else {
                            let mapped_end =
                                ensure_child_crt_allocation_mapped(child, pointer, size)?;
                            if pointer == 1
                                || pointer % 8 != 0
                                || pointer < CHILD_CRT_HEAP_BASE
                                || pointer
                                    .checked_add(size)
                                    .filter(|end| {
                                        *end <= CHILD_CRT_HEAP_LIMIT && *end <= mapped_end
                                    })
                                    .is_none()
                            {
                                return Err("child CRT malloc pointer verification failed".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT MALLOC pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" size={} pointer=0x{:08x} mapped_end=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    size,
                                    pointer,
                                    mapped_end,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = pointer;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_dllonexit = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "__dllonexit"
                    );
                    if is_crt_dllonexit {
                        let result = {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            child_dllonexit(
                                child,
                                process,
                                active_pid,
                                active_tid,
                                &running_module_name,
                                provider_id,
                                exit.registers.esp,
                            )?
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_initterm = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "_initterm"
                    );
                    if is_crt_initterm {
                        if child.initterm.is_some() {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT INITTERM FRONTIER pid={} tid={} reason=nested-initterm",
                                    active_pid, active_tid
                                ),
                            );
                            return Ok(());
                        }
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let caller_ret = frame[0];
                        let begin = frame[1];
                        let end = frame[2];
                        if running_module_name.eq_ignore_ascii_case("Storm.dll")
                            && caller_ret != 0x1503_630a
                        {
                            return Err(format!(
                                "Storm _initterm caller mismatch expected=0x1503630a actual=0x{caller_ret:08x}"
                            ));
                        }
                        if begin > end || begin % 4 != 0 || end % 4 != 0 {
                            return Err(format!(
                                "malformed child _initterm range begin=0x{begin:08x} end=0x{end:08x}"
                            ));
                        }
                        let bytes = end
                            .checked_sub(begin)
                            .ok_or_else(|| "child _initterm range underflow".to_owned())?;
                        if bytes % 4 != 0 {
                            return Err("child _initterm byte range is not pointer-aligned".into());
                        }
                        let entries = bytes / 4;
                        if entries > 65_536 {
                            return Err(format!(
                                "child _initterm range exceeds defensive bound entries={entries}"
                            ));
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT INITTERM pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} begin=0x{:08x} end=0x{:08x} entries={} caller_ret=0x{:08x}",
                                active_pid, active_tid, running_module_name, provider_id, begin, end, entries, caller_ret
                            ),
                        );
                        child.initterm = Some(ChildInitterm {
                            provider_resume_eip: exit.registers.eip,
                            provider_esp: exit.registers.esp,
                            begin,
                            cursor: begin,
                            end,
                            callbacks_invoked: 0,
                        });
                        match advance_child_initterm(child, &mut contexts[active])? {
                            InittermAdvance::CallbackScheduled | InittermAdvance::Complete => continue,
                        }
                    }
                    let is_reg_open_key_ex_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("ADVAPI32.dll")
                                && name == "RegOpenKeyExA"
                    );
                    if is_reg_open_key_ex_a {
                        let child_memory = X86Memory(&child.address_space);
                        let frame = decode_reg_open_key_ex_a(&child_memory, exit.registers.esp)?;
                        if frame.caller_ret != u32::from_le_bytes(caller_ret) {
                            return Err("RegOpenKeyExA caller return mismatch".into());
                        }
                        let subkey = frame.subkey.as_deref().unwrap_or("");
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY OPEN pid={} tid={} root=\"{}\" subkey=[redacted] subkey_bytes={} sam=0x{:08x}",
                                active_pid,
                                active_tid,
                                registry_root_name(frame.hkey),
                                subkey.len(),
                                frame.sam
                            ),
                        );
                        ensure_registry_loaded(&mut session).await?;
                        let registry = match &session.registry {
                            wc3::session::RegistryState::Ready(registry) => registry,
                            wc3::session::RegistryState::Unloaded => {
                                return Err("registry remained unloaded".into());
                            }
                        };
                        let start = registry.root(frame.hkey).or_else(|| {
                            session
                                .process(active_pid)
                                .and_then(|process| process.xp.registry_handle_node(frame.hkey))
                        });
                        let node = (frame.options == 0)
                            .then_some(start)
                            .flatten()
                            .and_then(|node| registry.child_path(node, subkey));
                        let (result, handle) = if let Some(node) = node {
                            let handle = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .open_registry_key(node, frame.sam)
                                .map_err(str::to_owned)?;
                            child
                                .address_space
                                .write(frame.result_ptr, &handle.to_le_bytes())
                                .map_err(|error| error.to_string())?;
                            (0u32, Some(handle))
                        } else {
                            (2u32, None)
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY OPEN RESULT pid={} exists={} result={} handle={}",
                                active_pid,
                                node.is_some() as u8,
                                result,
                                handle
                                    .map(|handle| format!("0x{handle:08x}"))
                                    .unwrap_or_else(|| "-".into())
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_virtual_alloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "VirtualAlloc"
                    );
                    if is_virtual_alloc {
                        let frame = decode_virtual_alloc_frame(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                        )?;
                        if frame.size == 0 {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error(87);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        let supported_reserve = frame.address == 0
                            && frame.size != 0
                            && frame.allocation_type == 0x0000_2000
                            && frame.protect == 0x01;
                        let supported_commit = frame.address != 0
                            && frame.size != 0
                            && frame.allocation_type == 0x0000_1000
                            && frame.protect == 0x04;
                        if !supported_reserve && !supported_commit {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                "WC3 CHILD VIRTUALALLOC FRONTIER pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} caller_ret=0x{:08x} address=0x{:08x} size=0x{:08x} allocation_type=0x{:08x} protect=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                frame.caller_ret,
                                frame.address,
                                frame.size,
                                frame.allocation_type,
                                frame.protect,
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                "WC3 CHILD VIRTUALALLOC FLAGS commit={} reserve={} top_down={} protect_name=\"{}\"",
                                (frame.allocation_type & 0x0000_1000 != 0) as u8,
                                (frame.allocation_type & 0x0000_2000 != 0) as u8,
                                (frame.allocation_type & 0x0010_0000 != 0) as u8,
                                virtual_alloc_protect_name(frame.protect),
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC FRONTIER reason=unsupported-observed-shape"
                                ),
                            );
                            return Ok(());
                        }
                        if supported_commit {
                            let request = {
                                let process = &session
                                    .process(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp;
                                match process.virtual_prepare_commit(frame.address, frame.size) {
                                    Ok(Some(request)) => request,
                                    Ok(None) => {
                                        session
                                            .process_mut(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .set_last_error(487);
                                        let mut registers = exit.registers;
                                        registers.eax = 0;
                                        contexts[active]
                                            .context
                                            .set_registers(registers)
                                            .map_err(|error| error.to_string())?;
                                        continue;
                                    }
                                    Err("VirtualAlloc overlapping commit") => {
                                        logl::log(level::IMPORTANT, format_args!(
                                            "WC3 CHILD VIRTUALALLOC FRONTIER reason=overlapping-commit"
                                        ));
                                        return Ok(());
                                    }
                                    Err(error) => return Err(error.into()),
                                }
                            };
                            child
                                .address_space
                                .map(
                                    request.base,
                                    usize::try_from(request.size)
                                        .map_err(|_| "VirtualAlloc commit size")?,
                                    Permissions::READ | Permissions::WRITE,
                                )
                                .map_err(|error| format!("map VirtualAlloc commit: {error}"))?;
                            let zeroes = vec![0; usize::try_from(request.size)
                                .map_err(|_| "VirtualAlloc zero size")?];
                            if child
                                .address_space
                                .write(request.base, &zeroes)
                                .map_err(|error| error.to_string())?
                                != zeroes.len()
                            {
                                return Err("short VirtualAlloc commit initialization".into());
                            }
                            let (reservations, reserve_next, committed_ranges, committed_bytes) = {
                                let process = &mut session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp;
                                process.virtual_finish_commit(request).map_err(str::to_owned)?;
                                let (reservations, reserve_next) = process.virtual_reservation_state();
                                let (committed_ranges, committed_bytes) = process.virtual_commit_state();
                                (reservations, reserve_next, committed_ranges, committed_bytes)
                            };
                            logl::log(level::IMPORTANT, format_args!(
                                "WC3 CHILD VIRTUALALLOC COMMIT pid={} tid={} address=0x{:08x} requested_size=0x{:08x} commit_size=0x{:08x} reservation_base=0x{:08x} reservation_size=0x{:08x} protect=PAGE_READWRITE permissions=RW guest_mapped=1 zero_initialized=1 return_eax=0x{:08x}",
                                active_pid, active_tid, frame.address, frame.size, request.size,
                                request.reservation_base, request.reservation_size, request.base
                            ));
                            logl::log(level::IMPORTANT, format_args!(
                                "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} committed_ranges={} committed_bytes={} reserve_next=0x{:08x}",
                                active_pid, reservations, committed_ranges, committed_bytes, reserve_next
                            ));
                            let mut registers = exit.registers;
                            registers.eax = request.base;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        let (reservation, reservation_count, reserve_next) = {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            let reservation = match process.virtual_reserve_null(frame.size) {
                                Ok(reservation) => reservation,
                                Err(_) => None,
                            };
                            if reservation.is_none() {
                                process.set_last_error(8);
                            }
                            let (reservation_count, reserve_next) = process.virtual_reservation_state();
                            (reservation, reservation_count, reserve_next)
                        };
                        let result = reservation.as_ref().map(|value| value.base).unwrap_or(0);
                        if let Some(reservation) = reservation {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC RESERVE pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" requested_address=0x00000000 requested_size=0x{:08x} reserved_base=0x{:08x} reserved_size=0x{:08x} allocation_type=MEM_RESERVE protect=PAGE_NOACCESS committed=0 guest_mapped=0",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    frame.size,
                                    reservation.base,
                                    reservation.size,
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} reserve_next=0x{:08x}",
                                    active_pid, reservation_count, reserve_next
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let operation = child_loader::provider_op(&provider);
                    if operation == child_loader::ProviderOp::ExitProcess {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let exit_code = frame[1];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD EXITPROCESS CALL pid={} tid={} during=\"{}\" exit_code=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                exit_code,
                                frame[0],
                            ),
                        );
                        let action = {
                            let mut child_memory = X86Memory(&child.address_space);
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .dispatch_provider_for_process(
                                    active_pid,
                                    active_tid,
                                    provider_id,
                                    exit.registers.esp,
                                    &mut child_memory,
                                )
                                .map_err(str::to_owned)?
                        };
                        let PersonalityAction::ExitProcess(dispatched_code) = action else {
                            return Err("ExitProcess child provider returned a non-exit action".into());
                        };
                        if dispatched_code != exit_code {
                            return Err("ExitProcess child provider exit code mismatch".into());
                        }
                        let threads_signaled = session
                            .objects
                            .values()
                            .filter(|object| {
                                matches!(object, SessionObject::Thread(thread) if thread.key.pid == active_pid)
                            })
                            .count();
                        let woken = session
                            .terminate_process(active_pid, exit_code)
                            .map_err(str::to_owned)?;
                        wait_deadlines.retain(|key, _| key.pid != active_pid);
                        resume_completed_waiters(
                            &woken,
                            "process-exit",
                            &mut contexts,
                            &mut wait_deadlines,
                        )?;
                        thread_calls.retain(|(pid, _), _| *pid != active_pid);
                        let before = contexts.len();
                        contexts.retain(|context| context.pid != active_pid);
                        let contexts_removed = before - contexts.len();
                        let terminated = pending_child
                            .take()
                            .ok_or_else(|| "ExitProcess child state missing".to_owned())?;
                        if terminated.pid != active_pid {
                            return Err("ExitProcess child state PID mismatch".into());
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROCESS EXIT pid={} exit_code=0x{:08x} contexts_removed={} threads_signaled={} waiters_woken={} address_space_dropped=1 dll_detach_callbacks=0",
                                active_pid,
                                exit_code,
                                contexts_removed,
                                threads_signaled,
                                woken.len(),
                            ),
                        );
                        if contexts.is_empty() {
                            return Ok(());
                        }
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        return Err("no runnable context after child ExitProcess".into());
                    }
                    if operation == child_loader::ProviderOp::RtlUnwind {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            5,
                        )?;
                        let caller_ret = frame[0];
                        let target_frame = frame[1];
                        let target_ip = frame[2];
                        let exception_record = frame[3];
                        let return_value = frame[4];
                        let current_head = read_seh_chain_head(
                            &child.address_space,
                            exit.registers.fs_base,
                        )?;
                        let target_relation =
                            classify_unwind_target(child, current_head, target_frame)?;
                        let target_location = (target_ip != 0)
                            .then(|| child_pc_owner(child, target_ip))
                            .flatten();
                        let (target_owner, target_rva) = if target_ip == 0 {
                            ("null", 0)
                        } else {
                            target_location.unwrap_or(("unknown", 0))
                        };
                        let exception_relation = if exception_record == 0 {
                            "null".to_owned()
                        } else if child
                            .seh
                            .as_ref()
                            .is_some_and(|seh| seh.exception_record_va == exception_record)
                        {
                            "active-seh".to_owned()
                        } else {
                            let mut header = [0; 20];
                            if child
                                .address_space
                                .read(exception_record, &mut header)
                                .map_err(|error| error.to_string())?
                                != header.len()
                            {
                                return Err("short RtlUnwind exception record header read".into());
                            }
                            format!(
                                "external(code=0x{:08x},flags=0x{:08x},next=0x{:08x},address=0x{:08x},parameters={})",
                                u32::from_le_bytes(header[0..4].try_into().unwrap()),
                                u32::from_le_bytes(header[4..8].try_into().unwrap()),
                                u32::from_le_bytes(header[8..12].try_into().unwrap()),
                                u32::from_le_bytes(header[12..16].try_into().unwrap()),
                                u32::from_le_bytes(header[16..20].try_into().unwrap()),
                            )
                        };
                        let (active_registration, active_next, active_handler, active_record, active_context, active_depth) = child
                            .seh
                            .as_ref()
                            .map(|seh| (
                                seh.registration,
                                seh.next_registration,
                                seh.handler,
                                seh.exception_record_va,
                                seh.context_va,
                                seh.depth,
                            ))
                            .unwrap_or((0, 0, 0, 0, 0, 0));
                        let active_target = child
                            .seh
                            .as_ref()
                            .is_some_and(|seh| seh.registration == target_frame);
                        let supported_current_head = target_frame != 0
                            && target_frame == current_head
                            && active_target
                            && target_ip != 0
                            && target_ip == caller_ret
                            && exception_record == 0
                            && target_location.is_some();
                        if supported_current_head {
                            let resumed = rtl_unwind_current_target_registers(
                                exit.registers,
                                exit.registers.esp,
                                target_ip,
                                return_value,
                            )
                            .map_err(str::to_owned)?;
                            contexts[active]
                                .context
                                .set_registers(resumed)
                                .map_err(|error| error.to_string())?;
                            logl::log(level::IMPORTANT, format_args!(
                                "WC3 CHILD RTLUNWIND CONTINUE pid={} tid={} target_frame=0x{:08x} target_ip=0x{:08x} target_owner={:?} target_rva=0x{:08x} return_value=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x} fs_head=0x{:08x} handlers_called=0 frames_popped=0",
                                active_pid, active_tid, target_frame, target_ip, target_owner,
                                target_rva, return_value, exit.registers.esp, resumed.esp,
                                current_head,
                            ));
                            continue;
                        }
                        let reason = if target_frame == 0 {
                            "exit-unwind"
                        } else if matches!(target_relation, UnwindTargetRelation::LaterRegistration(_)) {
                            "multi-frame-unwind"
                        } else if matches!(target_relation, UnwindTargetRelation::NotInChain) {
                            "invalid-target-frame"
                        } else if !active_target {
                            "target-not-active-seh-registration"
                        } else if exception_record != 0 {
                            "supplied-exception-record"
                        } else if target_ip != caller_ret {
                            "nonlocal-target-ip"
                        } else if target_location.is_none() {
                            "unknown-target-ip"
                        } else {
                            "unsupported-current-head-shape"
                        };
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD RTLUNWIND FRONTIER pid={} tid={} reason={} caller_ret=0x{:08x} target_frame=0x{:08x} target_relation={} target_relation_detail={:?} target_ip=0x{:08x} target_ip_owner={:?} target_ip_rva=0x{:08x} exception_record=0x{:08x} exception_relation={} return_value=0x{:08x} fs_head=0x{:08x} active_registration=0x{:08x} active_next=0x{:08x} active_handler=0x{:08x} active_record=0x{:08x} active_context=0x{:08x} active_depth={}",
                            active_pid, active_tid, reason, caller_ret, target_frame, target_relation.name(), target_relation,
                            target_ip, target_owner, target_rva, exception_record,
                            exception_relation, return_value, current_head,
                            active_registration, active_next, active_handler, active_record,
                            active_context, active_depth,
                        ));
                        return Ok(());
                    }
                    if operation == child_loader::ProviderOp::UnhandledExceptionFilter {
                        let exception_pointers = read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 2)?[1];
                        if exception_pointers == 0 { return Err("WC3 CHILD UEF FRONTIER reason=null-exception-pointers".into()); }
                        let mut pointers = [0; 8];
                        if child.address_space.read(exception_pointers, &mut pointers).map_err(|error| error.to_string())? != 8 { return Err("WC3 CHILD UEF FRONTIER reason=unreadable-exception-pointers".into()); }
                        let filter = session.process(active_pid).ok_or_else(|| "child process missing".to_owned())?.xp.unhandled_exception_filter();
                        if filter == 0 {
                            let result = session.process(active_pid).ok_or_else(|| "child process missing".to_owned())?.xp.complete_unhandled_exception_filter(None).map_err(str::to_owned)?;
                            let mut registers = exit.registers; registers.eax = result;
                            contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                            logl::log(level::IMPORTANT, format_args!("WC3 CHILD UEF RETURN pid={} tid={} source=default filter=0x00000000 result=0x{:08x} cleanup=4-by-thunk", active_pid, active_tid, result));
                            continue;
                        }
                        let Some((owner, rva)) = child_pc_owner(child, filter).map(|(owner, rva)| (owner.to_owned(), rva)) else { return Err(format!("WC3 CHILD UEF FRONTIER reason=unknown-filter-address filter=0x{filter:08x}")); };
                        let callback_esp = exit.registers.esp.checked_sub(8).ok_or("UEF callback stack underflow")?;
                        let frame = [thunk32::CHILD_UEF_RETURN_ADDRESS, exception_pointers]; let mut bytes=[0;8]; for (i,value) in frame.into_iter().enumerate(){bytes[i*4..i*4+4].copy_from_slice(&value.to_le_bytes());}
                        if child.address_space.write(callback_esp,&bytes).map_err(|error| error.to_string())? != 8 { return Err("short UEF callback frame write".into()); }
                        child.unhandled_filter_call = Some(ChildUnhandledFilterCall { provider_resume_eip: exit.registers.eip, provider_esp: exit.registers.esp, filter });
                        let mut registers=exit.registers; registers.eip=filter; registers.esp=callback_esp; contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        logl::log(level::IMPORTANT, format_args!("WC3 CHILD UEF FILTER CALL pid={} tid={} filter=0x{:08x} filter_owner={:?} filter_rva=0x{:08x} exception_pointers=0x{:08x} provider_esp=0x{:08x} callback_esp=0x{:08x}", active_pid,active_tid,filter,owner,rva,exception_pointers,exit.registers.esp,callback_esp));
                        continue;
                    }
                    if operation == child_loader::ProviderOp::LoadLibraryA {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let name_ptr = frame[1];
                        if name_ptr == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY FRONTIER pid={} tid={} during=\"{}\" kind=null-name caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                            return Ok(());
                        }
                        let requested = wc3::process::read_c_string(
                            &X86Memory(&child.address_space),
                            name_ptr,
                            260,
                        )
                        .map_err(|error| format!("LoadLibraryA module name: {error}"))?;
                        let existing = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .loaded_module_handle(&requested);
                        if let Some(handle) = existing {
                            let mut registers = exit.registers;
                            registers.eax = handle;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY RETURN pid={} tid={} during=\"{}\" requested={:?} handle=0x{:08x} already_loaded=1 cleanup=4-by-thunk",
                                    active_pid, active_tid, running_module_name, requested, handle,
                                ),
                            );
                            continue;
                        }
                        let listing = async_fs::list_dir(b"/common/Warcraft III")
                            .await
                            .map_err(|error| {
                                format!("list Warcraft III directory for LoadLibraryA: TRUEOSFS {error}")
                            })?;
                        if listing.truncated {
                            return Err("Warcraft III directory listing truncated".into());
                        }
                        let stored = child_loader::resolve_file(&listing, &requested)
                            .map_err(str::to_owned)?;
                        let kind = if stored.is_some() { "local-native" } else { "external" };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LOADLIBRARY FRONTIER pid={} tid={} during=\"{}\" requested={:?} kind={} stored={:?} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                requested,
                                kind,
                                stored,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        return Ok(());
                    }
                    if operation == child_loader::ProviderOp::GetProcAddress {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let hmodule = frame[1];
                        let selector = if frame[2] >> 16 == 0 {
                            ProcSelector::Ordinal(frame[2] as u16)
                        } else {
                            ProcSelector::Name(
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    frame[2],
                                    260,
                                )
                                .map_err(|error| format!("GetProcAddress selector: {error}"))?,
                            )
                        };
                        let provider_symbol = selector.provider_symbol();
                        let provider_module = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .external_provider_module_name(hmodule)
                            .map(str::to_owned);
                        if let Some(provider_module) = provider_module {
                            let import = child_loader::ProviderImport {
                                module: provider_module.clone(),
                                symbol: provider_symbol.clone(),
                                iat_rva: 0,
                            };
                            let operation = child_loader::provider_op(&import);
                            if !operation.is_modeled() {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .set_last_error(ERROR_PROC_NOT_FOUND);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD GETPROCADDRESS MISS pid={} tid={} module={:?} selector={:?} reason=provider-op-unmodeled error={}",
                                        active_pid, active_tid, provider_module, selector, ERROR_PROC_NOT_FOUND,
                                    ),
                                );
                                log_get_proc_address_continuation(
                                    child,
                                    active_pid,
                                    active_tid,
                                    &provider_module,
                                    frame[0],
                                    &selector,
                                    0,
                                );
                                let mut registers = exit.registers;
                                registers.eax = 0;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            let existing = session
                                .process(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_export_address(&provider_module, &provider_symbol);
                            let (address, source) = if let Some(address) = existing {
                                (address, "provider-existing")
                            } else {
                                let addresses = {
                                    let process = &mut session
                                        .process_mut(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp;
                                    install_child_provider_imports(child, process, vec![import])?
                                };
                                (addresses[0], "provider-new")
                            };
                            let mut registers = exit.registers;
                            registers.eax = address;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS RETURN pid={} tid={} module={:?} selector={:?} address=0x{:08x} source={} cleanup=8-by-thunk",
                                    active_pid, active_tid, provider_module, selector, address, source,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                &provider_module,
                                frame[0],
                                &selector,
                                address,
                            );
                            continue;
                        }

                        let image_and_module = if hmodule == child.image.image_base {
                            Some((&child.image, "War3.exe".to_owned()))
                        } else {
                            child.native_modules.iter().find_map(|native| {
                                (native.image.image_base == hmodule)
                                    .then(|| (&native.image, native.stored.clone()))
                            })
                        };
                        let Some((image, module)) = image_and_module else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS FRONTIER pid={} tid={} kind=unknown-hmodule handle=0x{:08x} selector={:?}",
                                    active_pid, active_tid, hmodule, selector,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                "<unknown-hmodule>",
                                frame[0],
                                &selector,
                                0,
                            );
                            return Ok(());
                        };
                        let export = image.exports.iter().find(|export| match &selector {
                            ProcSelector::Name(name) => export.name.as_deref() == Some(name),
                            ProcSelector::Ordinal(ordinal) => export.ordinal == u32::from(*ordinal),
                        });
                        let Some(export) = export else {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error(ERROR_PROC_NOT_FOUND);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS MISS pid={} tid={} module={:?} selector={:?} reason=export-not-found error={}",
                                    active_pid, active_tid, module, selector, ERROR_PROC_NOT_FOUND,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                &module,
                                frame[0],
                                &selector,
                                0,
                            );
                            continue;
                        };
                        let pe32::ExportTarget::Rva(rva) = &export.target else {
                            let pe32::ExportTarget::Forwarder(forwarder) = &export.target else {
                                unreachable!()
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS FRONTIER pid={} tid={} kind=native-forwarder module={:?} selector={:?} forwarder={:?}",
                                    active_pid, active_tid, module, selector, forwarder,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                &module,
                                frame[0],
                                &selector,
                                0,
                            );
                            return Ok(());
                        };
                        if *rva >= image.size_of_image {
                            return Err("GetProcAddress export RVA outside image".into());
                        }
                        let address = image
                            .image_base
                            .checked_add(*rva)
                            .ok_or_else(|| "GetProcAddress export VA overflow".to_owned())?;
                        let mut registers = exit.registers;
                        registers.eax = address;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETPROCADDRESS RETURN pid={} tid={} module={:?} selector={:?} address=0x{:08x} source=native-export cleanup=8-by-thunk",
                                active_pid, active_tid, module, selector, address,
                            ),
                        );
                        log_get_proc_address_continuation(
                            child,
                            active_pid,
                            active_tid,
                            &module,
                            frame[0],
                            &selector,
                            address,
                        );
                        continue;
                    }
                    if matches!(operation,
                        child_loader::ProviderOp::CreateEventA
                        | child_loader::ProviderOp::CreateMutexA
                        | child_loader::ProviderOp::ReleaseMutex
                        | child_loader::ProviderOp::CloseHandle
                        | child_loader::ProviderOp::WaitForSingleObject
                    ) {
                        let action = session.process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp.dispatch_provider_for_process_typed(
                                active_pid, active_tid, provider_id, exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            ).map_err(|error| error.to_string())?;
                        let result = match action {
                            PersonalityAction::Return(result) => result,
                            PersonalityAction::Session(request) => service_sync_request(
                                &mut session, active_pid, request, &mut contexts, &mut wait_deadlines,
                            )?,
                            PersonalityAction::Block(request) => {
                                if let Some(result) = session.poll_wait(&request).map_err(str::to_owned)? {
                                    logl::log(level::IMPORTANT, format_args!(
                                        "WC3 CHILD WAIT RETURN pid={} tid={} handle=0x{:08x} result=0x{:08x}",
                                        active_pid, active_tid, request.handles[0], result
                                    ));
                                    result
                                } else {
                                    session.block_wait(request.clone()).map_err(str::to_owned)?;
                                    if request.timeout != INFINITE {
                                        wait_deadlines.insert(request.key, RuntimeWait {
                                            deadline: tokio::time::Instant::now()
                                                + Duration::from_millis(request.timeout as u64),
                                            timeout_ms: request.timeout,
                                            handle: request.handles[0],
                                            resume_registers: exit.registers,
                                        });
                                    }
                                    logl::log(level::IMPORTANT, format_args!(
                                        "WC3 CHILD WAIT BLOCK pid={} tid={} handle=0x{:08x} timeout_ms={}",
                                        active_pid, active_tid, request.handles[0], request.timeout
                                    ));
                                    loop {
                                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                                            active = next;
                                            break;
                                        }
                                        if let Some(deadline) = wait_deadlines.values().map(|wait| wait.deadline).min() {
                                            tokio::time::sleep_until(deadline).await;
                                            expire_runtime_waits(&mut session, &mut contexts,
                                                &mut wait_deadlines, &mut previous_wait_timeout)?;
                                        } else {
                                            tokio::time::sleep(Duration::from_millis(8)).await;
                                        }
                                    }
                                    continue;
                                }
                            }
                            _ => return Err("unexpected child synchronization action".into()),
                        };
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD SYNC RETURN pid={} tid={} {} eax=0x{:08x} cleanup={}-by-thunk",
                            active_pid, active_tid, symbol, result, operation.stack_cleanup_bytes()
                        ));
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active].context.set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation.is_generic_process_local() {
                        let process_memory = matches!(
                            operation,
                            child_loader::ProviderOp::ReadProcessMemory
                                | child_loader::ProviderOp::WriteProcessMemory
                        )
                        .then(|| {
                            read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                6,
                            )
                        })
                        .transpose()?;
                        let set_file_attributes = if operation
                            == child_loader::ProviderOp::SetFileAttributesA
                        {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                3,
                            )?;
                            let [_, filename, attributes] = frame.as_slice()
                            else {
                                unreachable!("SetFileAttributesA frame has three words")
                            };
                            let path = if *filename == 0 {
                                "<null>".to_owned()
                            } else {
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    *filename,
                                    1024,
                                )
                                .map_err(|error| {
                                    format!("SetFileAttributesA filename: {error}")
                                })?
                            };
                            Some((path, *attributes))
                        } else {
                            None
                        };
                        let dispatch = {
                            let mut child_memory = X86Memory(&child.address_space);
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .dispatch_provider_for_process_typed_with_self_image(
                                    active_pid,
                                    active_tid,
                                    provider_id,
                                    exit.registers.esp,
                                    &mut child_memory,
                                    Some(child.self_image_bytes.as_slice()),
                                )
                        };
                        match dispatch {
                            Ok(PersonalityAction::Return(result)) => {
                                if let Some((path, attributes)) = set_file_attributes {
                                    let last_error = session
                                        .process(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .last_error();
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD SETFILEATTRIBUTESA path={path:?} attributes=0x{attributes:08x} exists={} result={} last_error={}",
                                            u8::from(result != 0),
                                            result,
                                            last_error,
                                        ),
                                    );
                                }
                                if let Some(frame) = process_memory {
                                    let [_, process, first, second, size, bytes_transferred] =
                                        frame.as_slice()
                                    else {
                                        unreachable!("process-memory frame has six words")
                                    };
                                    match operation {
                                        child_loader::ProviderOp::ReadProcessMemory => logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD READPROCESSMEMORY pid={} tid={} process=0x{:08x} source=0x{:08x} destination=0x{:08x} size={} bytes_read=0x{:08x} result={}",
                                                active_pid,
                                                active_tid,
                                                process,
                                                first,
                                                second,
                                                size,
                                                bytes_transferred,
                                                result,
                                            ),
                                        ),
                                        child_loader::ProviderOp::WriteProcessMemory => logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD WRITEPROCESSMEMORY pid={} tid={} process=0x{:08x} destination=0x{:08x} source=0x{:08x} size={} bytes_written=0x{:08x} result={}",
                                                active_pid,
                                                active_tid,
                                                process,
                                                first,
                                                second,
                                                size,
                                                bytes_transferred,
                                                result,
                                            ),
                                        ),
                                        _ => unreachable!("process-memory operation was classified before dispatch"),
                                    }
                                }
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                "WC3 CHILD PROVIDER RETURN pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eax=0x{:08x} cleanup={}-by-thunk",
                                        active_pid,
                                        active_tid,
                                        running_module_name,
                                        provider_id,
                                        provider.module,
                                        symbol,
                                        result,
                                        operation.stack_cleanup_bytes(),
                                    ),
                                );
                                let mut registers = exit.registers;
                                registers.eax = result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            Ok(_) => {
                                return Err("pure child provider requested a runtime effect".into());
                            }
                            Err(ProviderDispatchError::Unsupported) => {}
                            Err(ProviderDispatchError::Fault(error)) => {
                                return Err(format!("child provider semantic fault: {error}"));
                            }
                            Err(ProviderDispatchError::Frontier { api, detail }) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x} api=\"{}\" detail={:?}",
                                        active_pid,
                                        active_tid,
                                        running_module_name,
                                        provider_id,
                                        provider.module,
                                        symbol,
                                        exit.registers.eip,
                                        exit.registers.esp,
                                        u32::from_le_bytes(caller_ret),
                                        api,
                                        detail,
                                    ),
                                );
                                return Ok(());
                            }
                        }
                    }
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x}",
                            active_pid,
                            active_tid,
                            running_module_name,
                            provider_id,
                            provider.module,
                            symbol,
                            exit.registers.eip,
                            exit.registers.esp,
                            u32::from_le_bytes(caller_ret)
                        ),
                    );
                    return Ok(());
                }
                if exit.registers.eip == thunk32::THREAD_EXIT_AFTER_VMCALL {
                    if contexts[active].tid != LAUNCHER_TID {
                        let Some(next) = terminate_launcher_thread(
                            &mut session,
                            &mut contexts,
                            active,
                            exit.registers.eax,
                            thread_calls,
                            &mut wait_deadlines,
                        )? else {
                            return Ok(());
                        };
                        active = next;
                        continue;
                    }
                    let exited = contexts.remove(active);
                    session
                        .launcher_mut()
                        .xp
                        .exit_thread(exited.tid, exit.registers.eax)
                        .map_err(str::to_owned)?;
                    logl::log(
                        level::INFO,
                        format_args!(
                            "wc3: x86 ThreadProc exited tid={} code=0x{:08x}",
                            exited.tid, exit.registers.eax,
                        ),
                    );
                    if contexts.is_empty() {
                        return Ok(());
                    }
                    active %= contexts.len();
                    continue;
                }
                if exit.registers.eip == thunk32::GUEST_RETURN_AFTER_VMCALL {
                    let continuation = contexts[active]
                        .continuation
                        .take()
                        .ok_or_else(|| "guest return without continuation".to_owned())?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CALL_GUEST RETURN tid={} hwnd=0x{:08x} message=0x{:08x} wndproc=0x{:08x} result=0x{:08x}",
                            contexts[active].tid,
                            continuation.hwnd,
                            continuation.message,
                            continuation.wndproc,
                            exit.registers.eax
                        ),
                    );
                    if contexts[active].tid == 1 && continuation.hwnd == WINDOW_HANDLE_BASE {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CALL_GUEST RETURN tid=1 hwnd=0x{:08x} wndproc=0x{:08x} message=WM_PAINT result=0x{:08x}",
                                continuation.hwnd, continuation.wndproc, exit.registers.eax
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 ROOT WM_PAINT RETURN hwnd=0x{:08x}",
                                continuation.hwnd
                            ),
                        );
                    }
                    let mut registers = exit.registers;
                    registers.eip = continuation.import_resume_eip;
                    registers.esp = continuation.import_esp;
                    registers.eax = continuation.completion_eax;
                    contexts[active]
                        .context
                        .set_registers(registers)
                        .map_err(|error| error.to_string())?;
                    continue;
                }
                let import_id = exit.registers.eax;
                let import = session
                    .launcher()
                    .xp
                    .import(import_id)
                    .cloned()
                    .ok_or_else(|| format!("unknown import trap id={import_id}"))?;
                let thread_call = thread_calls
                    .entry((LAUNCHER_PID, contexts[active].tid))
                    .and_modify(|call| *call += 1)
                    .or_insert(1);
                let sequence = session.note();
                let process_call = session.launcher().xp.call_count + 1;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3[seq={sequence} p=launcher pid={} tid={} pcall={process_call} tcall={}] {}!{}",
                        LAUNCHER_PID,
                        contexts[active].tid,
                        *thread_call,
                        import.module,
                        import.symbol
                    ),
                );
                let call_kind = WinCall::from_import(&import);
                if call_kind == WinCall::BitBlt {
                    let frame = read_guest_words(&memory, exit.registers.esp, 10)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 BitBlt ret=0x{:08x} dst=0x{:08x} dst_xy={},{} size={}x{} src=0x{:08x} src_xy={},{} rop=0x{:08x}",
                            frame[0],
                            frame[1],
                            frame[2],
                            frame[3],
                            frame[4],
                            frame[5],
                            frame[6],
                            frame[7],
                            frame[8],
                            frame[9]
                        ),
                    );
                }
                if call_kind == WinCall::DefWindowProcA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                    if default_proc_messages.insert(frame[2]) {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 DefWindowProcA hwnd=0x{:08x} message=0x{:08x} wparam=0x{:08x} lparam=0x{:08x}",
                                frame[1], frame[2], frame[3], frame[4]
                            ),
                        );
                    }
                }
                if call_kind == WinCall::RegisterClassA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                    let fields = read_guest_words(&memory, frame[1], 10)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC0 tid={} ptr={:#010x}",
                            contexts[active].tid, frame[1]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC1 style={:#010x} wndproc={:#010x}",
                            fields[0], fields[1]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 RC2 cls_extra={} wnd_extra={}", fields[2], fields[3]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC3 instance={:#010x} icon={:#010x}",
                            fields[4], fields[5]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC4 cursor={:#010x} background={:#010x}",
                            fields[6], fields[7]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC5 menu_ptr={:#010x} class_ptr={:#010x}",
                            fields[8], fields[9]
                        ),
                    );
                    if fields[9] != 0 && fields[9] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, fields[9]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 RCCLASS {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 RCCLASS decode-failed ptr={:#010x}", fields[9]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 RCCLASS atom=0x{:04x}", fields[9] & 0xffff),
                        );
                    }
                    if fields[8] == 0 {
                        logl::log(level::IMPORTANT, format_args!("WC3 RCMENU <null>"));
                    } else if fields[8] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, fields[8]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 RCMENU {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 RCMENU decode-failed ptr={:#010x}", fields[8]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 RCMENU atom=0x{:04x}", fields[8] & 0xffff),
                        );
                    }
                }
                if call_kind == WinCall::CreateWindowExA {
                    let a = read_guest_words(&memory, exit.registers.esp, 13)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CW0 tid={} esp={:#010x} ret={:#010x}",
                            contexts[active].tid, exit.registers.esp, a[0]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW1 ex={:#010x} style={:#010x}", a[1], a[4]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW2 class_ptr={:#010x} title_ptr={:#010x}", a[2], a[3]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW3 x={} y={}", a[5] as i32, a[6] as i32),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW4 width={} height={}", a[7], a[8]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW5 parent={:#010x} menu={:#010x}", a[9], a[10]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW6 instance={:#010x} param={:#010x}", a[11], a[12]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWA a0..a4={:?}", &a[0..5]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWB a5..a8={:?}", &a[5..9]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWC a9..a12={:?}", &a[9..13]),
                    );
                    if a[2] != 0 && a[2] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, a[2]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 CWCLASS {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 CWCLASS decode-failed ptr={:#010x}", a[2]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 CWCLASS atom=0x{:04x}", a[2] & 0xffff),
                        );
                    }
                    if a[3] == 0 {
                        logl::log(level::IMPORTANT, format_args!("WC3 CWTITLE <null>"));
                    } else {
                        match diagnostic_ansi_string(&memory, a[3]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 CWTITLE {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 CWTITLE decode-failed ptr={:#010x}", a[3]),
                            ),
                        }
                    }
                }
                if contexts[active].tid == 2 && import.symbol == "TlsSetValue" {
                    let raw = read_guest_words(&memory, exit.registers.esp, 3)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 TID2 TlsSetValue esp=0x{:08x} return_address=0x{:08x} slot={} value=0x{:08x}",
                            exit.registers.esp, raw[0], raw[1], raw[2]
                        ),
                    );
                }
                if import.symbol == "DrawTextA" {
                    match read_guest_words(&memory, exit.registers.esp, 6) {
                        Ok(a) => {
                            logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 DT0 ret=0x{:08x} hdc=0x{:08x}", a[0], a[1]),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DT1 text_ptr=0x{:08x} count={}",
                                    a[2], a[3] as i32
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DT2 rect_ptr=0x{:08x} format=0x{:08x}",
                                    a[4], a[5]
                                ),
                            );
                            if (a[3] as i32) >= 0 {
                                match read_guest_bytes(&memory, a[2], a[3] as usize) {
                                    Ok(bytes) => {
                                        let text = wc3::ThisToThat::cp1252_to_string(&bytes);
                                        let digest = Sha256::digest(&bytes);
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 DTTEXT {:?} bytes_sha256={}",
                                                text,
                                                hex_digest(&digest)
                                            ),
                                        );
                                    }
                                    Err(error) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTTEXT decode-failed ptr=0x{:08x} count={} error={}",
                                            a[2], a[3], error
                                        ),
                                    ),
                                }
                            }
                            if a[4] != 0 {
                                match read_guest_words(&memory, a[4], 4) {
                                    Ok(rect) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTRECT in=[{},{},{},{}]",
                                            rect[0] as i32,
                                            rect[1] as i32,
                                            rect[2] as i32,
                                            rect[3] as i32
                                        ),
                                    ),
                                    Err(error) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTRECT decode-failed ptr=0x{:08x} error={}",
                                            a[4], error
                                        ),
                                    ),
                                }
                            }
                            let flags = a[5];
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DTFLAGS raw=0x{:08x} center={} vcenter={} wordbreak={} singleline={} calcrect={} noprefix={}",
                                    flags,
                                    u32::from(flags & 0x0001 != 0),
                                    u32::from(flags & 0x0004 != 0),
                                    u32::from(flags & 0x0010 != 0),
                                    u32::from(flags & 0x0020 != 0),
                                    u32::from(flags & 0x0400 != 0),
                                    u32::from(flags & 0x0800 != 0)
                                ),
                            );
                        }
                        Err(error) => logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 DT0 decode-failed esp=0x{:08x} error={}",
                                exit.registers.esp, error
                            ),
                        ),
                    }
                }
                if WinCall::from_import(&import) == WinCall::LoadImageA {
                    let raw = read_guest_words(&memory, exit.registers.esp, 7)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 LoadImageA RAW esp=0x{:08x} ret=0x{:08x} module=0x{:08x} name=0x{:08x} type={} cx={} cy={} flags=0x{:08x}",
                            exit.registers.esp,
                            raw[0],
                            raw[1],
                            raw[2],
                            raw[3],
                            raw[4],
                            raw[5],
                            raw[6]
                        ),
                    );
                }
                if call_kind == WinCall::MessageBoxA {
                    if active_message_box.is_some() {
                        return Err("nested MessageBoxA modal frontier".into());
                    }
                    let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                    let text = copy_message_box_ansi(&memory, frame[2])?;
                    let caption = copy_message_box_ansi(&memory, frame[3])?;
                    let buttons = message_box_buttons(frame[4]).ok_or_else(|| {
                        format!("MessageBoxA unsupported button type=0x{:x}", frame[4] & 0x0f)
                    })?;
                    let request = PendingMessageBox {
                        caller: active_key,
                        owner_hwnd: frame[1],
                        text,
                        caption,
                        style: frame[4],
                    };
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 MESSAGEBOXA OPEN pid={} tid={} owner=0x{:08x} text={:?} caption={:?} type=0x{:08x} buttons=[{}] topmost={} state=modal-blocked",
                            request.caller.pid,
                            request.caller.tid,
                            request.owner_hwnd,
                            request.text,
                            request.caption,
                            request.style,
                            buttons
                                .iter()
                                .map(|button| button.label)
                                .collect::<Vec<_>>()
                                .join(","),
                            u32::from(request.style & 0x0004_0000 != 0),
                        ),
                    );
                    *active_message_box = Some(open_message_box(request, buttons)?);
                    continue;
                }
                if WinCall::from_import(&import) == WinCall::Unsupported {
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 LAUNCHER UNSUPPORTED pid={} tid={} module={} symbol={} esp=0x{:08x} eip=0x{:08x} stack[0..16 dwords]={:?}",
                            active_key.pid,
                            active_key.tid,
                            import.module,
                            import.symbol,
                            exit.registers.esp,
                            exit.registers.eip,
                            read_guest_words(&memory, exit.registers.esp, 16)?
                        ),
                    );
                    return Ok(());
                }
                let result = session.launcher().xp.selected_bitmap(
                    if WinCall::from_import(&import) == WinCall::DeleteDC {
                        read_guest_words(&memory, exit.registers.esp, 2)?[1]
                    } else {
                        0
                    },
                );
                let delete_dc_selected = (WinCall::from_import(&import) == WinCall::DeleteDC)
                    .then_some((read_guest_words(&memory, exit.registers.esp, 2)?[1], result));
                let delete_object_before = if WinCall::from_import(&import) == WinCall::DeleteObject
                {
                    let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                    Some((
                        frame[1],
                        session.launcher().xp.bitmap_info(frame[1]),
                        session.launcher().xp.bitmap_stock(frame[1]),
                        session.launcher().xp.bitmap_selected_in_dc(frame[1]),
                    ))
                } else {
                    None
                };
                let draw_text_input = if WinCall::from_import(&import) == WinCall::DrawTextA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 6)?;
                    let rect = read_guest_words(&memory, frame[4], 4)?;
                    Some((frame, rect))
                } else {
                    None
                };
                let end_paint_input = if WinCall::from_import(&import) == WinCall::EndPaint {
                    let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                    let hdc = read_guest_words(&memory, frame[2], 1)?[0];
                    let target = match session.launcher().xp.dc_target(hdc) {
                        Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                        Some(None) => "MEMORY".to_owned(),
                        None => "UNKNOWN".to_owned(),
                    };
                    Some((frame, hdc, target))
                } else {
                    None
                };
                let result = session
                    .launcher_mut()
                    .xp
                    .dispatch_for_process(
                        LAUNCHER_PID,
                        contexts[active].tid,
                        import_id,
                        exit.registers.esp,
                        &mut memory,
                    )
                    .map_err(|error| {
                        format!(
                            "call #{} {}!{}: {error}",
                            session.launcher().xp.call_count,
                            import.module,
                            import.symbol
                        )
                    })?;
                let mut callback = None;
                let result = match result {
                    PersonalityAction::Return(value) => {
                        if WinCall::from_import(&import) == WinCall::TlsGetValue {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 TLSGETVALUE pid={} tid={} slot={} value=0x{:08x} result=0x{:08x}",
                                    active_key.pid,
                                    active_key.tid,
                                    frame[1],
                                    value,
                                    value,
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetLastError {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SETLASTERROR pid={} tid={} value=0x{:08x}",
                                    active_key.pid,
                                    active_key.tid,
                                    frame[1],
                                ),
                            );
                        }
                        if let Some((frame, hdc, target)) = end_paint_input.as_ref() {
                            if value == 1 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 EndPaint hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target={} retired=1",
                                        frame[1], frame[2], hdc, target
                                    ),
                                );
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 paint lifecycle active_paints={} paint_hdc_0x{:08x}_live={}",
                                        session.launcher().xp.active_paint_count(),
                                        hdc,
                                        u32::from(session.launcher().xp.gdi_live(*hdc))
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SetTextColor {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let new_color = frame[2] & 0x00ff_ffff;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetTextColor hdc=0x{:08x} target={} old=0x{:08x} new=0x{:08x} rgb=[{},{},{}]",
                                    frame[1],
                                    target,
                                    value,
                                    new_color,
                                    new_color & 0xff,
                                    (new_color >> 8) & 0xff,
                                    (new_color >> 16) & 0xff
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetBkColor {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let new_color = frame[2] & 0x00ff_ffff;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetBkColor hdc=0x{:08x} target={} old=0x{:08x} new=0x{:08x} rgb=[{},{},{}]",
                                    frame[1],
                                    target,
                                    value,
                                    new_color,
                                    new_color & 0xff,
                                    (new_color >> 8) & 0xff,
                                    (new_color >> 16) & 0xff
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetBkMode {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let mode_name = |mode| match mode {
                                1 => "TRANSPARENT",
                                2 => "OPAQUE",
                                _ => "UNKNOWN",
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetBkMode hdc=0x{:08x} target={} old={} new={} old_name={} new_name={}",
                                    frame[1],
                                    target,
                                    value,
                                    frame[2],
                                    mode_name(value),
                                    mode_name(frame[2])
                                ),
                            );
                            if let Some((text_color, bk_color, bk_mode)) =
                                session.launcher().xp.text_state(frame[1])
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 TEXT DC hdc=0x{:08x} text_color=0x{:08x} bk_color=0x{:08x} bk_mode={}",
                                        frame[1],
                                        text_color,
                                        bk_color,
                                        mode_name(bk_mode)
                                    ),
                                );
                            }
                        }
                        if let Some((handle, Some(info), Some(stock), selected_in_dc)) =
                            delete_object_before
                        {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteObject handle=0x{:08x} kind=BITMAP stock={} selected_in_dc={} bits_va=0x{:08x} bits_len={}",
                                    handle,
                                    u32::from(stock),
                                    u32::from(selected_in_dc),
                                    info.bits_va,
                                    info.bits_len
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteObject complete handle=0x{:08x} bitmap_live={} palette_0x57437003_live={} stock_bitmap_live={}",
                                    handle,
                                    u32::from(session.launcher().xp.gdi_live(handle)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7003)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7f01))
                                ),
                            );
                        }
                        if let Some((hdc, Some(selected))) = delete_dc_selected {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteDC hdc=0x{:08x} kind=MEMORY_DC selected_bitmap=0x{:08x}",
                                    hdc, selected
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteDC complete hdc=0x{:08x} remaining_bitmap_0x57437001={} remaining_stock_bitmap={} ",
                                    hdc,
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7001)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7f01))
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::CreatePalette && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let header = read_guest_bytes(&memory, frame[1], 4)?;
                            let entries = u16::from_le_bytes([header[2], header[3]]) as usize;
                            let exact = read_guest_bytes(&memory, frame[1] + 4, entries * 4)?;
                            let digest = Sha256::digest(&exact);
                            let allocation =
                                session.launcher().xp.allocation_size(frame[1]).unwrap_or(0);
                            let version = u16::from_le_bytes([header[0], header[1]]);
                            let entry0 = exact.get(..4).unwrap_or(&[]);
                            let entry255 = exact.get(255 * 4..256 * 4).unwrap_or(&[]);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CreatePalette LOGPALETTE ptr=0x{:08x} version=0x{:04x} entries={} required_bytes={} allocation_bytes={} entry0={:?} entry255={:?} palette_sha256={} hpalette=0x{:08x}",
                                    frame[1],
                                    version,
                                    entries,
                                    4 + entries * 4,
                                    allocation,
                                    entry0,
                                    entry255,
                                    hex_digest(&digest),
                                    value
                                ),
                            );
                            if let Some((stored_version, stored_entries)) =
                                session.launcher().xp.palette_info(value)
                            {
                                logl::log(
                                    level::INFO,
                                    format_args!(
                                        "wc3: logical palette admitted version=0x{:04x} entries={}",
                                        stored_version, stored_entries
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::GetDIBColorTable {
                            let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                            if value != 0 {
                                let bytes =
                                    read_guest_bytes(&memory, frame[4], value as usize * 4)?;
                                let digest = Sha256::digest(&bytes);
                                if let Some(bitmap) =
                                    session.launcher().xp.selected_bitmap(frame[1])
                                {
                                    if let Some(info) = session.launcher().xp.bitmap_info(bitmap) {
                                        let source_offset = (info.height.unsigned_abs() as usize
                                            - 1)
                                            * info.row_stride as usize;
                                        let mut source_index = [0; 1];
                                        memory
                                            .read(
                                                info.bits_va + source_offset as u32,
                                                &mut source_index,
                                            )
                                            .map_err(str::to_owned)?;
                                        let palette_offset = source_index[0] as usize * 4;
                                        let visual = bytes
                                            .get(palette_offset..palette_offset + 3)
                                            .ok_or_else(|| {
                                                "palette index outside returned table".to_owned()
                                            })?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 GetDIBColorTable hdc=0x{:08x} bitmap=0x{:08x} start={} requested={} copied={} output=0x{:08x} entry0={:?} entry255={:?} palette_sha256={} top_left_index={} top_left_rgb=[{},{},{}]",
                                                frame[1],
                                                bitmap,
                                                frame[2],
                                                frame[3],
                                                value,
                                                frame[4],
                                                bytes.get(..4).unwrap_or(&[]),
                                                bytes.get(255 * 4..256 * 4).unwrap_or(&[]),
                                                hex_digest(&digest),
                                                source_index[0],
                                                visual[2],
                                                visual[1],
                                                visual[0]
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SelectObject {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            if let Some(info) = session.launcher().xp.bitmap_info(frame[2]) {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SelectObject hdc=0x{:08x} new=0x{:08x} old=0x{:08x} new_type=BITMAP width={} height={} bpp={} bits_va=0x{:08x}",
                                        frame[1],
                                        frame[2],
                                        value,
                                        info.width,
                                        info.height,
                                        info.bit_count,
                                        info.bits_va
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::CreateCompatibleDC {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let source = frame[1];
                            let (source_kind, source_hwnd) = if source == 0 {
                                ("DISPLAY", None)
                            } else {
                                match session.launcher().xp.dc_target(source) {
                                    Some(Some(hwnd)) => ("WINDOW_PAINT", Some(hwnd)),
                                    Some(None) => ("MEMORY_DC", None),
                                    None => ("UNKNOWN", None),
                                }
                            };
                            if let Some((selected, width, height, bpp)) =
                                session.launcher().xp.compatible_dc_info(value)
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CreateCompatibleDC source=0x{:08x} source_kind={} source_hwnd={} compatibility=DISPLAY hdc=0x{:08x} target=MEMORY selected_bitmap=0x{:08x} selected_bitmap_shape={}x{}x{}",
                                        source,
                                        source_kind,
                                        source_hwnd.map_or_else(
                                            || "-".to_owned(),
                                            |hwnd| format!("0x{hwnd:08x}")
                                        ),
                                        value,
                                        selected,
                                        width,
                                        height,
                                        bpp
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SelectPalette && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 4)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let entries =
                                session.launcher().xp.palette_entries(frame[2]).unwrap_or(0);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SelectPalette hdc=0x{:08x} target={} new=0x{:08x} old=0x{:08x} force_background={} entries={}",
                                    frame[1],
                                    target,
                                    frame[2],
                                    value,
                                    frame[3] != 0,
                                    entries
                                ),
                            );
                            if let (Some(paint_palette), Some(memory_bitmap)) = (
                                session.launcher().xp.selected_palette(0x5743_7004),
                                session.launcher().xp.selected_bitmap(0x5743_7008),
                            ) {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 palette state paint_hdc=0x57437004 selected_palette=0x{:08x} memory_hdc=0x57437008 selected_bitmap=0x{:08x}",
                                        paint_palette, memory_bitmap
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::RealizePalette {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let palette = session.launcher().xp.selected_palette(frame[1]);
                            let entries = palette
                                .and_then(|handle| session.launcher().xp.palette_entries(handle))
                                .unwrap_or(0);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 RealizePalette hdc=0x{:08x} target={} palette={} entries={} first_realization={} mapped={}",
                                    frame[1],
                                    target,
                                    palette
                                        .map_or_else(|| "-".to_owned(), |v| format!("0x{v:08x}")),
                                    entries,
                                    u32::from(value != 0 && value != u32::MAX),
                                    value
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::GetObjectA && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 4)?;
                            if let Some(info) = session.launcher().xp.bitmap_info(frame[1]) {
                                let mut first = [0; 1];
                                memory
                                    .read(info.bits_va, &mut first)
                                    .map_err(str::to_owned)?;
                                let rgba = &info.rgba[..4];
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GetObjectA BITMAP hbitmap=0x{:08x} buffer=0x{:08x} bytes={} width={} height={} width_bytes={} planes={} bpp={} bits_va=0x{:08x} bits_len={} first_index={} first_visual_rgba=[{},{},{},{}]",
                                        frame[1],
                                        frame[3],
                                        frame[2],
                                        info.width,
                                        info.height,
                                        info.row_stride,
                                        info.planes,
                                        info.bit_count,
                                        info.bits_va,
                                        info.bits_len,
                                        first[0],
                                        rgba[0],
                                        rgba[1],
                                        rgba[2],
                                        rgba[3]
                                    ),
                                );
                            }
                        }
                        value
                    }
                    PersonalityAction::Session(SessionRequest::LoadImage(request)) => {
                        let layout = dib_layout(&request.dib).map_err(str::to_owned)?;
                        let bits_va = session
                            .launcher_mut()
                            .xp
                            .allocate_gdi_bits(layout.bits_len)
                            .map_err(str::to_owned)?;
                        let mapped_len = (layout.bits_len + 0xfff) & !0xfff;
                        address_space
                            .map(bits_va, mapped_len, Permissions::READ | Permissions::WRITE)
                            .map_err(|error| format!("map DIB bits: {error}"))?;
                        let bits_end = layout
                            .pixel_offset
                            .checked_add(layout.bits_len)
                            .ok_or_else(|| "DIB bits range overflow".to_owned())?;
                        address_space
                            .write(bits_va, &request.dib[layout.pixel_offset..bits_end])
                            .map_err(|error| format!("write DIB bits: {error}"))?;
                        let bmp = bmp_file_from_dib(&request.dib).map_err(str::to_owned)?;
                        let decoded = match trueos::vmedia::decode(
                            trueos::vmedia::ImageFormat::Bmp,
                            &bmp,
                        )
                        .await
                        {
                            Ok(decoded) => decoded,
                            Err(error) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 BITMAP DECODE_REJECTED resource={} width={} height={} planes={} bpp={} compression={} dib_bytes={} decoder_error={}",
                                        request.resource_id,
                                        request.width,
                                        request.height,
                                        request.planes,
                                        request.bit_count,
                                        request.compression,
                                        request.dib.len(),
                                        error
                                    ),
                                );
                                return Ok(());
                            }
                        };
                        let value = session
                            .launcher_mut()
                            .xp
                            .admit_bitmap(request, decoded.rgba, bits_va, layout)
                            .map_err(str::to_owned)?;
                        let info = session
                            .launcher()
                            .xp
                            .bitmap_info(value)
                            .ok_or_else(|| "admitted bitmap disappeared".to_owned())?;
                        let rgba_top_left = info
                            .rgba
                            .get(..4)
                            .ok_or_else(|| "decoded bitmap has no top-left pixel".to_owned())?;
                        let digest = Sha256::digest(&info.rgba);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BITMAP resource={} width={} height={} planes={} bpp={} compression={} dib_bytes={} rgba_bytes={} rgba_top_left=[{},{},{},{}] rgba_sha256={} hbitmap=0x{:08x}",
                                info.resource_id,
                                info.width,
                                info.height,
                                info.planes,
                                info.bit_count,
                                info.compression,
                                info.dib_bytes,
                                info.rgba.len(),
                                rgba_top_left[0],
                                rgba_top_left[1],
                                rgba_top_left[2],
                                rgba_top_left[3],
                                hex_digest(&digest),
                                value
                            ),
                        );
                        value
                    }
                    PersonalityAction::WindowBlit(request) => {
                        let digest = Sha256::digest(&request.rgba);
                        window_rgba.insert(request.hwnd, request.rgba.clone());
                        let frame = frames
                            .get_mut(&request.hwnd)
                            .ok_or_else(|| "BitBlt destination frame missing".to_owned())?;
                        frame
                            .begin(rgba(0, 0, 0, 255))
                            .and_then(|()| frame.write_opaque_rgba8(&request.rgba))
                            .and_then(|()| {
                                frame.publish(Damage::full(request.width, request.height))
                            })
                            .map_err(|error| format!("publish WC3 BitBlt: {error:?}"))?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BitBlt SRCCOPY dst_hdc=0x{:08x} dst_hwnd=0x{:08x} dst=[{},{} {}x{}] src_hdc=0x{:08x} src_bitmap=0x{:08x} src=[0,0] rop=0x00cc0020 bits_va=0x{:08x} bpp=8 bottom_up={} rgba_bytes={} rgba_sha256={}",
                                request.dst_hdc,
                                request.hwnd,
                                request.dst_x,
                                request.dst_y,
                                request.width,
                                request.height,
                                request.src_hdc,
                                request.source_bitmap,
                                request.bits_va,
                                u32::from(request.bottom_up),
                                request.rgba.len(),
                                hex_digest(&digest)
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 BLIT hwnd=0x{:08x} width={} height={} source_bitmap=0x{:08x} published=1",
                                request.hwnd, request.width, request.height, request.source_bitmap
                            ),
                        );
                        if !*blit_checkpoint_done {
                            while trueos::vshell::attached_read_byte().is_some() {}
                            logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 UI4 BLIT LIVE confirm=anykey"),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 UI4 BLIT STOPPED awaiting operator input"),
                            );
                            let confirm = loop {
                                trueos::vsys::poll_once();
                                if let Some(byte) = trueos::vshell::attached_read_byte() {
                                    break byte;
                                }
                                trueos::vsys::sleep_ms(8);
                            };
                            *blit_checkpoint_done = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 UI4 BLIT RESUME byte=0x{:02x}", confirm),
                            );
                        }
                        1
                    }
                    PersonalityAction::WindowText(request) => {
                        let backing = window_rgba
                            .get(&request.hwnd)
                            .ok_or_else(|| "DrawTextA window backing missing".to_owned())?;
                        let frame = frames
                            .get_mut(&request.hwnd)
                            .ok_or_else(|| "DrawTextA destination frame missing".to_owned())?;
                        let expected_bytes = (frame.width() as usize)
                            .checked_mul(frame.height() as usize)
                            .and_then(|pixels| pixels.checked_mul(4))
                            .ok_or_else(|| "DrawTextA frame size overflow".to_owned())?;
                        if backing.len() != expected_bytes {
                            return Err("DrawTextA window backing size mismatch".into());
                        }
                        frame
                            .begin(rgba(0, 0, 0, 255))
                            .map_err(|error| format!("begin WC3 DrawTextA frame: {error:?}"))?;
                        frame
                            .write_opaque_rgba8(backing)
                            .map_err(|error| format!("restore WC3 DrawTextA backing: {error:?}"))?;
                        let row = SceneTextRow {
                            text: request.text.as_str(),
                            x: request.rect[0] as f32,
                            y: request.rect[1] as f32,
                            font_pixels: request.height as f32,
                        };
                        frame
                            .stamp_text_scene(
                                Font::Default,
                                (frame.width(), frame.height()),
                                rgba(240, 200, 0, 255),
                                core::slice::from_ref(&row),
                            )
                            .map_err(|error| format!("stamp WC3 DrawTextA text: {error:?}"))?;
                        let damage = Damage::full(frame.width(), frame.height());
                        loop {
                            match frame.publish(damage) {
                                Ok(()) => break,
                                Err(ui4_scene::Error::Busy) => {
                                    trueos::vsys::poll_once();
                                    trueos::vsys::sleep_ms(1);
                                }
                                Err(error) => {
                                    return Err(format!("publish WC3 DrawTextA text: {error:?}"));
                                }
                            }
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 TEXT hwnd=0x{:08x} hdc=0x{:08x} rect=[{},{},{},{}] height={} published=1",
                                request.hwnd,
                                request.hdc,
                                request.rect[0],
                                request.rect[1],
                                request.rect[2],
                                request.rect[3],
                                request.height
                            ),
                        );
                        16
                    }
                    PersonalityAction::Session(SessionRequest::CreateProcess(request)) => {
                        let frame = request.frame;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BLUEPRINT FRONTIER: CreateProcessA call #{} esp=0x{:08x} ret=0x{:08x} command_line=0x{:08x} startup=0x{:08x} process_info=0x{:08x}",
                                session.launcher().xp.call_count,
                                exit.registers.esp,
                                frame.return_address,
                                frame.command_line,
                                frame.startup_info,
                                frame.process_information,
                            ),
                        );
                        let child_bytes = async_fs::read_file(b"/common/Warcraft III/War3.exe")
                            .await
                            .map_err(|error| {
                                format!(
                                    "read /common/Warcraft III/War3.exe: TRUEOSFS error {error}"
                                )
                            })?;
                        let child = pe32::parse(&child_bytes).map_err(str::to_owned)?;
                        let child_file_bytes = Arc::new(child_bytes);
                        let created = session.create_child();
                        // CREATE_SUSPENDED was not requested.  The logical
                        // child thread is runnable, but its native entry is
                        // held at the loader frontier until dependencies are
                        // prepared.
                        session.enqueue(ThreadKey {
                            pid: created.pid,
                            tid: created.tid,
                        });
                        memory
                            .write(
                                frame.process_information,
                                &created.process_handle.to_le_bytes(),
                            )
                            .map_err(str::to_owned)?;
                        memory
                            .write(
                                frame.process_information + 4,
                                &created.thread_handle.to_le_bytes(),
                            )
                            .map_err(str::to_owned)?;
                        memory
                            .write(frame.process_information + 8, &created.pid.to_le_bytes())
                            .map_err(str::to_owned)?;
                        memory
                            .write(frame.process_information + 12, &created.tid.to_le_bytes())
                            .map_err(str::to_owned)?;
                        log_child_handles(&session, created.pid);
                        let child_address_space =
                            AddressSpace::create().map_err(|error| error.to_string())?;
                        child_address_space
                            .map(
                                PROCESS_DATA_VA,
                                0x1000,
                                Permissions::READ | Permissions::WRITE,
                            )
                            .map_err(|error| format!("map child process data: {error}"))?;
                        let written = child_address_space
                            .write(PROCESS_DATA_VA, CHILD_COMMAND_LINE)
                            .map_err(|error| format!("write child command line: {error}"))?;
                        if written != CHILD_COMMAND_LINE.len() {
                            return Err("short child command-line write".into());
                        }
                        let environment_written = child_address_space
                            .write(ENVIRONMENT_BLOCK_VA, &[0, 0, 0, 0])
                            .map_err(|error| format!("write child environment block: {error}"))?;
                        if environment_written != 4 {
                            return Err("short child environment-block write".into());
                        }
                        *pending_child = Some(PendingChild {
                            pid: created.pid,
                            tid: created.tid,
                            image: child,
                            self_image_bytes: child_file_bytes,
                            native_modules: Vec::new(),
                            address_space: child_address_space,
                            crt_heap_mapped_end: CHILD_CRT_HEAP_BASE,
                            win_heap_mapped_end: CHILD_WIN_HEAP_BASE,
                            provider_thunk_bytes: 0,
                            static_load_reserved: 0,
                            initterm: None,
                            cipow: None,
                            cipow_diagnostic_logged: false,
                            seh_handler_dumped: false,
                            seh: None,
                            unhandled_filter_call: None,
                            repeated_null_call: None,
                            repeated_divide_fault: None,
                            single_step_count: 0,
                            scan_progress: None,
                            scan_heartbeat_source: None,
                            dword_scan_watch: None,
                            table_checkpoint_capture: None,
                            loader: ChildLoaderState {
                                prepared: false,
                                native_requests: Vec::new(),
                                next_native: 0,
                            },
                            execution: ChildExecutionState::Loader,
                        });
                        logl::log(
                            level::INFO,
                            format_args!(
                                "wc3: CreateProcessA succeeded pid={} tid={} process_handle=0x{:08x} thread_handle=0x{:08x}",
                                created.pid,
                                created.tid,
                                created.process_handle,
                                created.thread_handle
                            ),
                        );
                        1
                    }
                    PersonalityAction::Session(SessionRequest::GetExitCodeProcess(request)) => {
                        match session.get_exit_code_process(request.pid, request.handle) {
                            Ok((target_pid, exit_code)) => {
                                memory
                                    .write(request.exit_code_pointer, &exit_code.to_le_bytes())
                                    .map_err(str::to_owned)?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GETEXITCODEPROCESS pid={} tid={} handle=0x{:08x} target_pid={} exit_code=0x{:08x} result=1",
                                        request.pid,
                                        request.tid,
                                        request.handle,
                                        target_pid,
                                        exit_code,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(request.pid)
                                    .ok_or_else(|| "GetExitCodeProcess caller missing".to_owned())?
                                    .xp
                                    .set_last_error(6);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GETEXITCODEPROCESS pid={} tid={} handle=0x{:08x} target_pid=- exit_code=- result=0 error=6",
                                        request.pid,
                                        request.tid,
                                        request.handle,
                                    ),
                                );
                                0
                            }
                        }
                    }
                    PersonalityAction::Session(SessionRequest::CreateEvent(request)) => {
                        let (handle, already_exists) = session.create_event(LAUNCHER_PID, request);
                        if handle != 0 {
                            session.launcher_mut().xp.set_last_error(if already_exists { 183 } else { 0 });
                        }
                        handle
                    }
                    PersonalityAction::Session(request @ SessionRequest::CreateMutex { .. })
                    | PersonalityAction::Session(request @ SessionRequest::ReleaseMutex { .. }) => {
                        service_sync_request(&mut session, LAUNCHER_PID, request, &mut contexts, &mut wait_deadlines)?
                    }
                    PersonalityAction::Session(SessionRequest::SetEvent { pid, tid, handle }) => {
                        match session.set_event(pid, handle) {
                            Ok(outcome) => {
                                let waiters_woken = outcome.woken.len();
                                resume_completed_waiters(
                                    &outcome.woken,
                                    "set-event",
                                    &mut contexts,
                                    &mut wait_deadlines,
                                )?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SETEVENT pid={} tid={} handle=0x{:08x} manual_reset={} was_signaled={} waiters_woken={} result=1",
                                        pid,
                                        tid,
                                        handle,
                                        outcome.manual_reset as u8,
                                        outcome.was_signaled as u8,
                                        waiters_woken,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(pid)
                                    .ok_or_else(|| "SetEvent caller missing".to_owned())?
                                    .xp
                                    .set_last_error(6);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SETEVENT pid={} tid={} handle=0x{:08x} result=0 error=6",
                                        pid, tid, handle,
                                    ),
                                );
                                0
                            }
                        }
                    }
                    PersonalityAction::Session(SessionRequest::CreateWindow(request)) => {
                        let hwnd = session
                            .create_window(request.clone())
                            .map_err(str::to_owned)?;
                        let window = session.windows.get(&hwnd).ok_or("created window missing")?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CreateWindowExA hwnd=0x{:08x} owner=pid{}/tid{} class={:?} title={:?} wndproc=0x{:08x} geometry={},{} {}x{} style=0x{:08x} ex_style=0x{:08x} visible={} ui4_frame=0",
                                hwnd,
                                window.owner.pid,
                                window.owner.tid,
                                window.class,
                                window.title,
                                window.wndproc,
                                window.x,
                                window.y,
                                window.width,
                                window.height,
                                window.style,
                                window.ex_style,
                                u32::from(window.visible)
                            ),
                        );
                        hwnd
                    }
                    PersonalityAction::Session(SessionRequest::ShowWindow {
                        pid: _,
                        hwnd,
                        show,
                    }) => session.show_window(hwnd, show).map_err(str::to_owned)?,
                    PersonalityAction::Session(SessionRequest::DestroyWindow { pid, hwnd }) => {
                        let frame_present = frames.contains_key(&hwnd);
                        match session.destroy_window(pid, hwnd) {
                            Ok(result) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 DESTROYWINDOW pid={} tid={} hwnd=0x{:08x} frame_present={} focused={} result=1",
                                        pid,
                                        active_key.tid,
                                        hwnd,
                                        frame_present as u8,
                                        result.was_focused as u8,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(pid)
                                    .ok_or_else(|| "DestroyWindow caller missing".to_owned())?
                                    .xp
                                    .set_last_error(1400);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 DESTROYWINDOW pid={} tid={} hwnd=0x{:08x} frame_present={} focused=- result=0 error=1400",
                                        pid,
                                        active_key.tid,
                                        hwnd,
                                        frame_present as u8,
                                    ),
                                );
                                0
                            }
                        }
                    }
                    PersonalityAction::Session(SessionRequest::UpdateWindow { pid: _, hwnd }) => {
                        let pending = session.update_window(hwnd).map_err(str::to_owned)?;
                        if pending != 0 {
                            let window = session.windows.get(&hwnd).ok_or("window disappeared")?;
                            callback = Some(GuestCall {
                                address: window.wndproc,
                                arguments: [hwnd, 0x000f, 0, 0],
                                completion_eax: 1,
                            });
                        }
                        pending
                    }
                    PersonalityAction::Session(SessionRequest::BeginPaint {
                        pid,
                        hwnd,
                        paint_struct,
                    }) => {
                        let (width, height) = session
                            .begin_paint_window(pid, hwnd)
                            .map_err(str::to_owned)?;
                        let hdc = session
                            .process_mut(pid)
                            .ok_or("BeginPaint process missing")?
                            .xp
                            .begin_paint(hwnd, paint_struct, width, height, &mut memory)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BeginPaint hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target=WINDOW client={}x{} rcPaint=[0,0,{},{}] erase=0",
                                hwnd, paint_struct, hdc, width, height, width, height
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 PAINT HANDLES hwnd=0x{:08x} hdc=0x{:08x} distinct={}",
                                hwnd,
                                hdc,
                                u32::from(hwnd != hdc)
                            ),
                        );
                        hdc
                    }
                    PersonalityAction::Session(SessionRequest::SetFocus { pid: _, hwnd }) => {
                        session.set_focus(hwnd).map_err(str::to_owned)?
                    }
                    PersonalityAction::Session(SessionRequest::CloseHandle { pid, handle }) => {
                        if session.close_handle(pid, handle) {
                            1
                        } else {
                            session.launcher_mut().xp.set_last_error(6);
                            0
                        }
                    }
                    PersonalityAction::Block(request) => {
                        let is_single = import.symbol == "WaitForSingleObject";
                        if is_single {
                            let handle = request.handles[0];
                            let repeated_wait_timeout = *previous_wait_timeout
                                == Some((request.key, handle, request.timeout));
                            let description = session.describe_handle(request.key.pid, handle);
                            let state = session.event_state(request.key.pid, handle);
                            if !repeated_wait_timeout {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WaitForSingleObject pid={} tid={} ret=0x{:08x} handle=0x{:08x} timeout={} object={}",
                                        request.key.pid,
                                        request.key.tid,
                                        request.return_address,
                                        handle,
                                        request.timeout,
                                        description
                                    ),
                                );
                                if let Some((manual_reset, signaled)) = state {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WaitForSingleObject state manual_reset={} signaled={}",
                                            u32::from(manual_reset),
                                            u32::from(signaled)
                                        ),
                                    );
                                }
                            }
                            if let Some(wait_result) =
                                session.poll_wait(&request).map_err(str::to_owned)?
                            {
                                if wait_result != WAIT_TIMEOUT {
                                    *previous_wait_timeout = None;
                                }
                                let consumed = state
                                    .map(|(manual_reset, signaled)| signaled && !manual_reset)
                                    .unwrap_or(false);
                                if wait_result == WAIT_OBJECT_0 {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT SIGNALED pid={} tid={} handle=0x{:08x} auto_reset_consumed={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            handle,
                                            u32::from(consumed),
                                            wait_result
                                        ),
                                    );
                                } else if wait_result == WAIT_TIMEOUT && !repeated_wait_timeout {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT TIMEOUT pid={} tid={} handle=0x{:08x} elapsed_ms=0 result=0x{:08x}",
                                            request.key.pid, request.key.tid, handle, wait_result
                                        ),
                                    );
                                } else if wait_result == WAIT_FAILED {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT FAILED pid={} tid={} handle=0x{:08x} result=0x{:08x}",
                                            request.key.pid, request.key.tid, handle, wait_result
                                        ),
                                    );
                                }
                                let mut registers = exit.registers;
                                registers.eax = wait_result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                if !repeated_wait_timeout {
                                    logl::log(
                                        level::INFO,
                                        format_args!(
                                            "wc3: return #{} KERNEL32.dll!WaitForSingleObject eax=0x{:08x}",
                                            session.launcher().xp.call_count,
                                            wait_result
                                        ),
                                    );
                                }
                                continue;
                            }
                            if !repeated_wait_timeout {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WAIT BLOCK pid={} tid={} handle=0x{:08x} timeout_ms={}",
                                        request.key.pid, request.key.tid, handle, request.timeout
                                    ),
                                );
                            }
                        } else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 RAW #90 WaitForMultipleObjects esp=0x{:08x} ret=0x{:08x} count={} handles_ptr=0x{:08x} wait_all={} timeout=0x{:08x}",
                                    exit.registers.esp,
                                    request.return_address,
                                    request.count,
                                    request.handles_pointer,
                                    request.wait_all,
                                    request.timeout,
                                ),
                            );
                            if request.handles_pointer != 0 && request.count <= 1024 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 RAW #90 handles handle0=0x{:08x} handle1=0x{:08x}",
                                        request.handles[0], request.handles[1],
                                    ),
                                );
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 WAIT MULTIPLE pid={} tid={} call=#{} ret=0x{:08x} count={} handles_ptr=0x{:08x} handle0=0x{:08x} handle1=0x{:08x} wait_all={} timeout=0x{:08x}",
                                    LAUNCHER_PID,
                                    request.key.tid,
                                    session.launcher().xp.call_count,
                                    request.return_address,
                                    request.count,
                                    request.handles_pointer,
                                    request.handles[0],
                                    request.handles[1],
                                    request.wait_all,
                                    request.timeout,
                                ),
                            );
                            logl::log(
                                level::INFO,
                                format_args!(
                                    "wc3: wait handle0 {}",
                                    session.describe_handle(LAUNCHER_PID, request.handles[0])
                                ),
                            );
                            logl::log(
                                level::INFO,
                                format_args!(
                                    "wc3: wait handle1 {}",
                                    session.describe_handle(LAUNCHER_PID, request.handles[1])
                                ),
                            );
                            if request.count == 0 || request.count > 2 || request.wait_all > 1 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WAIT FRONTIER reason=unsupported-shape count={} wait_all={}",
                                        request.count, request.wait_all,
                                    ),
                                );
                                return Ok(());
                            }
                            if let Some(wait_result) =
                                session.poll_wait(&request).map_err(str::to_owned)?
                            {
                                let index = wait_result.saturating_sub(WAIT_OBJECT_0);
                                if wait_result >= WAIT_OBJECT_0 && index < request.count {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT SIGNALED pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} index={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.handles[0],
                                            request.handles[1],
                                            request.count,
                                            request.wait_all,
                                            index,
                                            wait_result,
                                        ),
                                    );
                                } else if wait_result == WAIT_TIMEOUT {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT TIMEOUT pid={} tid={} count={} wait_all={} elapsed_ms=0 result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.count,
                                            request.wait_all,
                                            wait_result,
                                        ),
                                    );
                                } else {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT FAILED pid={} tid={} count={} wait_all={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.count,
                                            request.wait_all,
                                            wait_result,
                                        ),
                                    );
                                }
                                let mut registers = exit.registers;
                                registers.eax = wait_result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 WAIT BLOCK pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} timeout_ms={}",
                                    request.key.pid,
                                    request.key.tid,
                                    request.handles[0],
                                    request.handles[1],
                                    request.count,
                                    request.wait_all,
                                    request.timeout,
                                ),
                            );
                        }
                        session.block_wait(request.clone()).map_err(str::to_owned)?;
                        if request.timeout != INFINITE {
                            wait_deadlines.insert(
                                request.key,
                                RuntimeWait {
                                    deadline: tokio::time::Instant::now()
                                        + Duration::from_millis(request.timeout as u64),
                                    timeout_ms: request.timeout,
                                    handle: request.handles[0],
                                    resume_registers: exit.registers,
                                },
                            );
                        }
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        if let Some(key) =
                            session.runnable.iter().find(|key| key.pid != LAUNCHER_PID)
                        {
                            let child = pending_child
                                .as_mut()
                                .filter(|child| child.pid == key.pid && child.tid == key.tid)
                                .ok_or_else(|| "runnable child missing pending image".to_owned())?;
                            let listing = async_fs::list_dir(b"/common/Warcraft III")
                                .await
                                .map_err(|error| {
                                    format!("list Warcraft III directory: TRUEOSFS {error}")
                                })?;
                            if listing.truncated {
                                return Err("Warcraft III directory listing truncated".into());
                            }
                            let loaded =
                                session
                                    .assets
                                    .preload_war3(&listing)
                                    .await
                                    .map_err(|error| {
                                        logl::log(
                                            level::ERROR,
                                            format_args!(
                                    "WC3 RAM ASSET FAILED asset=\"war3.mpq\" error={error:?}"
                                ),
                                        );
                                        error
                                    })?;
                            if loaded {
                                let asset = session.assets.war3_mpq().expect("resident MPQ");
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 RAM ASSET READY asset=\"war3.mpq\" stored=\"{}\" bytes={} backing=host-ram guest_mapped=0 copies=1",
                                        asset.stored_path(),
                                        asset.len(),
                                    ),
                                );
                            }
                            let surface = child_loader::prepare(&mut child.image, &listing)
                                .map_err(str::to_owned)?;
                            child.loader.native_requests = surface.native.clone();
                            child.loader.prepared = true;
                            map_child_image(&child.address_space, &child.image)?;
                            log_child_slot_xrefs(child, WAR3_REPEATED_NULL_CALL_SLOT);
                            log_child_slot_xrefs(child, WAR3_SCAN_INDEX);
                            log_child_slot_xrefs(child, WAR3_SCAN_SOURCE);
                            log_child_slot_xrefs(child, WAR3_SCAN_BOUND);
                            log_child_slot_xrefs(child, WAR3_SCAN_COUNT);
                            let mut bytes = [0u8; 0xb0];
                            if child.address_space.read(WAR3_HOTLOOP_START, &mut bytes).ok()
                                == Some(bytes.len())
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD HOTLOOP CODE start=0x{WAR3_HOTLOOP_START:08x} bytes=\"{}\"",
                                        diagnostic_hex_bytes(&bytes),
                                    ),
                                );
                            }
                            map_child_thunks(&child.address_space, &surface.thunks)?;
                            map_child_controls(&child.address_space)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD PROVIDERS READY pid={} modules={} imports={} named={} ordinal={} thunk_base=0x{:08x} thunk_bytes={} patched_iat={}",
                                    child.pid,
                                    surface.external_modules,
                                    surface.imports.len(),
                                    surface.named,
                                    surface.ordinal,
                                    thunk32::THUNK_BASE,
                                    surface.thunks.len(),
                                    surface.imports.len(),
                                ),
                            );
                            let initial_provider_thunk_bytes = surface.thunks.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .try_install_provider_surface(
                                    surface.imports,
                                    surface.thunks,
                                    surface.providers,
                                )
                                .map_err(str::to_owned)?;
                            child.provider_thunk_bytes = initial_provider_thunk_bytes;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXECUTION ROUTER READY pid={} provider_imports={} provider_thunk_bytes={} control_base=0x{:08x} provider_namespace=child memory_space=child",
                                    child.pid,
                                    session
                                        .process(child.pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .provider_import_count(),
                                    4096,
                                    thunk32::CHILD_CONTROL_BASE,
                                ),
                            );
                            let native = child
                                .loader
                                .native_requests
                                .get(child.loader.next_native)
                                .cloned()
                                .ok_or_else(|| {
                                    "child has no local native direct module".to_owned()
                                })?;
                            let path = format!("/common/Warcraft III/{}", native.stored);
                            let bytes = async_fs::read_file(path.as_bytes())
                                .await
                                .map_err(|error| format!("read {path}: TRUEOSFS error {error}"))?;
                            let image = pe32::parse(&bytes).map_err(str::to_owned)?;
                            child
                                .address_space
                                .map(
                                    image.image_base,
                                    image.image.len(),
                                    Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
                                )
                                .map_err(|_| "preferred-base-unavailable".to_owned())?;
                            let written = child
                                .address_space
                                .write(image.image_base, &image.image)
                                .map_err(|error| error.to_string())?;
                            if written != image.image.len() {
                                return Err("short native image write".into());
                            }
                            let storm_imports: Vec<_> = image
                                .imports
                                .iter()
                                .map(|import| child_loader::ProviderImport {
                                    module: import.module.clone(),
                                    symbol: match &import.symbol {
                                        pe32::ImportSymbol::Name(name) => {
                                            child_loader::ProviderSymbol::Name(name.clone())
                                        }
                                        pe32::ImportSymbol::Ordinal(ordinal) => {
                                            child_loader::ProviderSymbol::Ordinal(*ordinal)
                                        }
                                    },
                                    iat_rva: import.iat_rva,
                                })
                                .collect();
                            let external_modules = storm_imports
                                .iter()
                                .map(|import| import.module.as_str())
                                .collect::<std::collections::HashSet<_>>()
                                .len();
                            let (addresses, old_bytes, new_bytes, updated_from, updated) = session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .append_provider_imports(storm_imports)
                                .map_err(str::to_owned)?;
                            let provider_thunks_total = session
                                .process(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_import_count();
                            child.provider_thunk_bytes = new_bytes;
                            if new_bytes > old_bytes {
                                child
                                    .address_space
                                    .map(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        new_bytes - old_bytes,
                                        Permissions::READ
                                            | Permissions::WRITE
                                            | Permissions::EXECUTE,
                                    )
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER THUNK GROW pid={} old_bytes={} new_bytes={}",
                                        child.pid, old_bytes, new_bytes
                                    ),
                                );
                            }
                            let existing_update =
                                old_bytes.saturating_sub(updated_from).min(updated.len());
                            if existing_update != 0 {
                                child
                                    .address_space
                                    .write(
                                        thunk32::THUNK_BASE + updated_from as u32,
                                        &updated[..existing_update],
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            if existing_update < updated.len() {
                                child
                                    .address_space
                                    .write(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        &updated[existing_update..],
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            for (import, address) in image.imports.iter().zip(addresses) {
                                child
                                    .address_space
                                    .write(
                                        image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE IMPORTS READY pid={} module=\"{}\" external_modules={} external_imports={} patched_iat={} provider_thunks_total={} thunk_bytes={}",
                                    child.pid,
                                    native.stored,
                                    external_modules,
                                    image.imports.len(),
                                    image.imports.len(),
                                    provider_thunks_total,
                                    new_bytes
                                ),
                            );
                            let export_named = image
                                .exports
                                .iter()
                                .filter(|export| export.name.is_some())
                                .count();
                            let export_forwarders = image
                                .exports
                                .iter()
                                .filter(|export| {
                                    matches!(export.target, pe32::ExportTarget::Forwarder(_))
                                })
                                .count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE EXPORTS module=\"{}\" exports={} named={} ordinal_only={} forwarders={}",
                                    native.stored,
                                    image.exports.len(),
                                    export_named,
                                    image.exports.len() - export_named,
                                    export_forwarders
                                ),
                            );
                            let parent_imports: Vec<_> = child
                                .image
                                .imports
                                .iter()
                                .filter(|import| {
                                    import.module.eq_ignore_ascii_case(&native.requested)
                                })
                                .collect();
                            let mut resolved = 0usize;
                            let mut parent_named = 0usize;
                            let mut parent_ordinal = 0usize;
                            for import in &parent_imports {
                                let export = match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => {
                                        parent_named += 1;
                                        image.exports.iter().find(|export| export.name.as_deref() == Some(name.as_str()))
                                    }
                                    pe32::ImportSymbol::Ordinal(ordinal) => {
                                        parent_ordinal += 1;
                                        image.exports.iter().find(|export| export.ordinal == u32::from(*ordinal))
                                    }
                                }.ok_or_else(|| match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" symbol=\"{}\"", native.stored, name),
                                    pe32::ImportSymbol::Ordinal(ordinal) => format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" ordinal={}", native.stored, ordinal),
                                })?;
                                let pe32::ExportTarget::Rva(rva) = &export.target else {
                                    let forwarder = match &export.target {
                                        pe32::ExportTarget::Forwarder(value) => value,
                                        _ => unreachable!(),
                                    };
                                    return Err(format!(
                                        "WC3 CHILD NATIVE EXPORT FORWARDER FRONTIER parent=\"War3.exe\" module=\"{}\" forwarder=\"{}\"",
                                        native.stored, forwarder
                                    ));
                                };
                                if *rva >= image.size_of_image {
                                    return Err("Storm export RVA outside image".into());
                                }
                                let address = image
                                    .image_base
                                    .checked_add(*rva)
                                    .ok_or_else(|| "Storm export VA overflow".to_owned())?;
                                child
                                    .address_space
                                    .write(
                                        child.image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(child.image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("War3 Storm IAT readback mismatch".into());
                                }
                                resolved += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE BIND parent=\"War3.exe\" module=\"{}\" imports={} resolved={} named={} ordinal={} forwarded=0",
                                    native.stored,
                                    parent_imports.len(),
                                    resolved,
                                    parent_named,
                                    parent_ordinal
                                ),
                            );
                            if resolved != parent_imports.len() {
                                return Err("incomplete War3 Storm binding".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MAP pid={} module=\"{}\" preferred_base=0x{:08x} mapped_base=0x{:08x} size=0x{:08x} relocation_delta=0 relocations_applied=0",
                                    child.pid,
                                    native.stored,
                                    image.image_base,
                                    image.image_base,
                                    image.size_of_image
                                ),
                            );
                            log_native_child_image(
                                &native.requested,
                                &native.stored,
                                &image,
                                &listing,
                            )
                            .map_err(str::to_owned)?;
                            let native_base = image.image_base;
                            let native_imports = image.imports.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .register_native_module(
                                    &native.requested,
                                    &native.stored,
                                    native_base,
                                )
                                .map_err(str::to_owned)?;
                            child.native_modules.push(PendingNativeModule {
                                requested: native.requested,
                                stored: native.stored.clone(),
                                image,
                                initialized: false,
                            });
                            let initialized = child
                                .native_modules
                                .last()
                                .ok_or_else(|| "stored Storm missing".to_owned())?
                                .initialized;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MODULE READY pid={} module=\"{}\" base=0x{:08x} imports_bound={} parent_imports_resolved={} initialized={}",
                                    child.pid,
                                    native.stored,
                                    native_base,
                                    native_imports,
                                    resolved,
                                    initialized as u8
                                ),
                            );
                            child.loader.next_native += 1;
                            let next_native = child
                                .loader
                                .native_requests
                                .get(child.loader.next_native)
                                .ok_or_else(|| "no unresolved native child module".to_owned())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE ADVANCE pid={} from=\"{}\" to=\"{}\"",
                                    child.pid, native.stored, next_native.stored
                                ),
                            );
                            let mss = next_native.clone();
                            let mss_path = format!("/common/Warcraft III/{}", mss.stored);
                            let mss_bytes =
                                async_fs::read_file(mss_path.as_bytes()).await.map_err(
                                    |error| format!("read {mss_path}: TRUEOSFS error {error}"),
                                )?;
                            let mss_image = pe32::parse(&mss_bytes).map_err(str::to_owned)?;
                            child
                                .address_space
                                .map(
                                    mss_image.image_base,
                                    mss_image.image.len(),
                                    Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
                                )
                                .map_err(|_| "preferred-base-unavailable".to_owned())?;
                            let mss_written = child
                                .address_space
                                .write(mss_image.image_base, &mss_image.image)
                                .map_err(|error| error.to_string())?;
                            if mss_written != mss_image.image.len() {
                                return Err("short Mss image write".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MAP pid={} module=\"{}\" preferred_base=0x{:08x} mapped_base=0x{:08x} size=0x{:08x} relocation_delta=0 relocations_applied=0",
                                    child.pid,
                                    mss.stored,
                                    mss_image.image_base,
                                    mss_image.image_base,
                                    mss_image.size_of_image
                                ),
                            );
                            let mss_export_named = mss_image
                                .exports
                                .iter()
                                .filter(|export| export.name.is_some())
                                .count();
                            let mss_forwarders = mss_image
                                .exports
                                .iter()
                                .filter(|export| {
                                    matches!(export.target, pe32::ExportTarget::Forwarder(_))
                                })
                                .count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE EXPORTS module=\"{}\" exports={} named={} ordinal_only={} forwarders={}",
                                    mss.stored,
                                    mss_image.exports.len(),
                                    mss_export_named,
                                    mss_image.exports.len() - mss_export_named,
                                    mss_forwarders
                                ),
                            );
                            let mss_parent_imports: Vec<_> = child
                                .image
                                .imports
                                .iter()
                                .filter(|import| import.module.eq_ignore_ascii_case(&mss.requested))
                                .collect();
                            let mut mss_resolved = 0usize;
                            for import in &mss_parent_imports {
                                let pe32::ImportSymbol::Name(name) = &import.symbol else {
                                    return Err("unexpected ordinal War3 Mss import".into());
                                };
                                let export = mss_image.exports.iter().find(|export| export.name.as_deref() == Some(name.as_str()))
                                    .ok_or_else(|| format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" symbol=\"{}\"", mss.stored, name))?;
                                let pe32::ExportTarget::Rva(rva) = &export.target else {
                                    let pe32::ExportTarget::Forwarder(forwarder) = &export.target
                                    else {
                                        unreachable!()
                                    };
                                    return Err(format!(
                                        "WC3 CHILD NATIVE EXPORT FORWARDER FRONTIER parent=\"War3.exe\" module=\"{}\" symbol=\"{}\" forwarder=\"{}\"",
                                        mss.stored, name, forwarder
                                    ));
                                };
                                if *rva >= mss_image.size_of_image {
                                    return Err("Mss export RVA outside image".into());
                                }
                                let address = mss_image
                                    .image_base
                                    .checked_add(*rva)
                                    .ok_or_else(|| "Mss export VA overflow".to_owned())?;
                                child
                                    .address_space
                                    .write(
                                        child.image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(child.image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("War3 Mss IAT readback mismatch".into());
                                }
                                mss_resolved += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE BIND parent=\"War3.exe\" module=\"{}\" imports={} resolved={} named={} ordinal=0 forwarded=0",
                                    mss.stored,
                                    mss_parent_imports.len(),
                                    mss_resolved,
                                    mss_parent_imports.len()
                                ),
                            );
                            if mss_resolved != mss_parent_imports.len() {
                                return Err("incomplete War3 Mss binding".into());
                            }
                            let mss_providers: Vec<_> = mss_image
                                .imports
                                .iter()
                                .map(|import| child_loader::ProviderImport {
                                    module: import.module.clone(),
                                    symbol: match &import.symbol {
                                        pe32::ImportSymbol::Name(name) => {
                                            child_loader::ProviderSymbol::Name(name.clone())
                                        }
                                        pe32::ImportSymbol::Ordinal(ordinal) => {
                                            child_loader::ProviderSymbol::Ordinal(*ordinal)
                                        }
                                    },
                                    iat_rva: import.iat_rva,
                                })
                                .collect();
                            let mss_external_modules = mss_providers
                                .iter()
                                .map(|import| import.module.as_str())
                                .collect::<std::collections::HashSet<_>>()
                                .len();
                            let (mss_addresses, old_bytes, new_bytes, updated_from, updated) =
                                session
                                    .process_mut(child.pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .append_provider_imports(mss_providers)
                                    .map_err(str::to_owned)?;
                            child.provider_thunk_bytes = new_bytes;
                            if new_bytes > old_bytes {
                                child
                                    .address_space
                                    .map(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        new_bytes - old_bytes,
                                        Permissions::READ
                                            | Permissions::WRITE
                                            | Permissions::EXECUTE,
                                    )
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER THUNK GROW pid={} old_bytes={} new_bytes={}",
                                        child.pid, old_bytes, new_bytes
                                    ),
                                );
                            }
                            let existing_update =
                                old_bytes.saturating_sub(updated_from).min(updated.len());
                            if existing_update != 0 {
                                child
                                    .address_space
                                    .write(
                                        thunk32::THUNK_BASE + updated_from as u32,
                                        &updated[..existing_update],
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            if existing_update < updated.len() {
                                child
                                    .address_space
                                    .write(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        &updated[existing_update..],
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            for (import, address) in mss_image.imports.iter().zip(mss_addresses) {
                                child
                                    .address_space
                                    .write(
                                        mss_image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(mss_image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("Mss provider IAT readback mismatch".into());
                                }
                            }
                            let mss_provider_total = session
                                .process(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_import_count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE IMPORTS READY pid={} module=\"{}\" external_modules={} external_imports={} patched_iat={} provider_thunks_total={} thunk_bytes={}",
                                    child.pid,
                                    mss.stored,
                                    mss_external_modules,
                                    mss_image.imports.len(),
                                    mss_image.imports.len(),
                                    mss_provider_total,
                                    new_bytes
                                ),
                            );
                            let mut winmm_total = 0usize;
                            let mut winmm_named = 0usize;
                            let mut winmm_ordinal = 0usize;
                            for import in mss_image
                                .imports
                                .iter()
                                .filter(|import| import.module.eq_ignore_ascii_case("WINMM.dll"))
                            {
                                let index = winmm_total;
                                let (symbol, kind) = match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => {
                                        winmm_named += 1;
                                        (name.clone(), "name")
                                    }
                                    pe32::ImportSymbol::Ordinal(value) => {
                                        winmm_ordinal += 1;
                                        (format!("#{value}"), "ordinal")
                                    }
                                };
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD WINMM IMPORT index={} symbol=\"{}\" iat_rva=0x{:08x} kind={}",
                                        index, symbol, import.iat_rva, kind
                                    ),
                                );
                                winmm_total += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WINMM SURFACE module=\"Mss32.dll\" imports={} named={} ordinal={}",
                                    winmm_total, winmm_named, winmm_ordinal
                                ),
                            );
                            log_native_child_image(
                                &mss.requested,
                                &mss.stored,
                                &mss_image,
                                &listing,
                            )
                            .map_err(str::to_owned)?;
                            let mss_base = mss_image.image_base;
                            let mss_imports = mss_image.imports.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .register_native_module(&mss.requested, &mss.stored, mss_base)
                                .map_err(str::to_owned)?;
                            child.native_modules.push(PendingNativeModule {
                                requested: mss.requested,
                                stored: mss.stored.clone(),
                                image: mss_image,
                                initialized: false,
                            });
                            let initialized = child
                                .native_modules
                                .last()
                                .ok_or_else(|| "stored Mss missing".to_owned())?
                                .initialized;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MODULE READY pid={} module=\"{}\" base=0x{:08x} imports_bound={} parent_imports_resolved={} initialized={}",
                                    child.pid,
                                    mss.stored,
                                    mss_base,
                                    mss_imports,
                                    mss_resolved,
                                    initialized as u8
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE LOAD COMPLETE pid={} modules={} initialized=0",
                                    child.pid,
                                    child.native_modules.len()
                                ),
                            );
                            let storm_index = child
                                .native_modules
                                .iter()
                                .position(|module| module.stored.eq_ignore_ascii_case("Storm.dll"))
                                .ok_or_else(|| "loaded Storm missing".to_owned())?;
                            child.execution = ChildExecutionState::DllInitReady {
                                native_index: storm_index,
                            };
                            let storm = child
                                .native_modules
                                .get(storm_index)
                                .ok_or_else(|| "loaded Storm missing".to_owned())?;
                            let storm_name = storm.stored.clone();
                            let storm_base = storm.image.image_base;
                            let storm_entry = storm
                                .image
                                .image_base
                                .checked_add(storm.image.entry_rva)
                                .ok_or_else(|| "Storm entry overflow".to_owned())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXECUTION STATE pid={} tid={} state=dll-init-ready module=\"{}\" context_created=0",
                                    child.pid, child.tid, storm_name
                                ),
                            );
                            let child_key = ThreadKey {
                                pid: child.pid,
                                tid: child.tid,
                            };
                            if context_index(&contexts, child_key).is_some() {
                                return Err("child primary context already exists".into());
                            }
                            let child_context = create_child_primary_context(child, storm_index)?;
                            let child_esp = STACK_TOP - 0x10;
                            let child_teb = thread_teb_va(child.tid)?;
                            let reserved = STACK_TOP - 0x20;
                            contexts.push(child_context);
                            if contexts
                                .iter()
                                .filter(|context| context.key() == child_key)
                                .count()
                                != 1
                            {
                                return Err("child primary context insertion mismatch".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CONTEXT READY pid={} tid={} state=dll-init-ready module=\"{}\" context_created=1 started=0 scheduled=0 eip=0x{:08x} esp=0x{:08x} teb=0x{:08x} stack_base=0x{:08x} stack_top=0x{:08x} return_va=0x{:08x}",
                                    child.pid,
                                    child.tid,
                                    storm_name,
                                    storm_entry,
                                    child_esp,
                                    child_teb,
                                    STACK_BASE,
                                    STACK_TOP,
                                    thunk32::CHILD_DLL_RETURN_ADDRESS
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL FRAME pid={} tid={} module=\"{}\" return=0x{:08x} hinst=0x{:08x} reason=1 reserved=0x{:08x}",
                                    child.pid,
                                    child.tid,
                                    storm_name,
                                    thunk32::CHILD_DLL_RETURN_ADDRESS,
                                    storm_base,
                                    reserved
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SCHEDULER READY pid={} tid={} context_identity=thread-key runnable_selection=process-aware wait_resume=process-aware control_routing=process-aware context_created=1",
                                    child.pid, child.tid
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT FRONTIER pid={} tid={} module=\"{}\" entry_va=0x{:08x} reason=context-ready-not-scheduled",
                                    child.pid, child.tid, storm_name, storm_entry,
                                ),
                            );
                            let child_context_index = context_index(&contexts, child_key)
                                .ok_or_else(|| {
                                    "child primary context missing after insertion".to_owned()
                                })?;
                            let matching_contexts = contexts
                                .iter()
                                .filter(|context| context.key() == child_key)
                                .count();
                            if matching_contexts != 1 {
                                return Err("child primary context is not unique".into());
                            }
                            if contexts[child_context_index].started {
                                return Err("child primary context unexpectedly started".into());
                            }
                            if child.native_modules[storm_index].initialized {
                                return Err(
                                    "Storm unexpectedly initialized before scheduling".into()
                                );
                            }
                            verify_child_primary_context(
                                child,
                                &contexts[child_context_index],
                                storm_index,
                            )?;
                            let (_, _, scheduled_entry) =
                                begin_child_dll_init(child).map_err(str::to_owned)?;
                            if scheduled_entry != storm_entry {
                                return Err("child DLL scheduling entry mismatch".into());
                            }
                            let runnable_count = session
                                .runnable
                                .iter()
                                .filter(|key| **key == child_key)
                                .count();
                            if runnable_count != 1 {
                                return Err(
                                    "child runnable queue entry missing or duplicated".into()
                                );
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT SCHEDULE pid={} tid={} module=\"{}\" state=dll-init-running eip=0x{:08x} esp=0x{:08x}",
                                    child.pid, child.tid, storm_name, storm_entry, child_esp
                                ),
                            );
                            let selected = pop_runnable_context(&mut session, &contexts)
                                .ok_or_else(|| {
                                    "child runnable context was not selected".to_owned()
                                })?;
                            if contexts[selected].key() != child_key {
                                return Err("ordinary scheduler selected non-child context".into());
                            }
                            active = selected;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SCHEDULED pid={} tid={} module=\"{}\" started=0",
                                    child.pid, child.tid, storm_name
                                ),
                            );
                            continue;
                        }
                        let Some(deadline) =
                            wait_deadlines.values().map(|wait| wait.deadline).min()
                        else {
                            return Err("no runnable thread after launcher block".into());
                        };
                        tokio::time::sleep_until(deadline).await;
                        expire_runtime_waits(
                            &mut session,
                            &mut contexts,
                            &mut wait_deadlines,
                            &mut previous_wait_timeout,
                        )?;
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        return Err("expired wait did not produce runnable launcher context".into());
                    }
                    PersonalityAction::CallGuest(call) => {
                        callback = Some(call);
                        0
                    }
                    PersonalityAction::ExitThread(exit_code) => {
                        let Some(next) = terminate_launcher_thread(
                            &mut session,
                            &mut contexts,
                            active,
                            exit_code,
                            thread_calls,
                            &mut wait_deadlines,
                        )? else {
                            return Ok(());
                        };
                        active = next;
                        continue;
                    }
                    PersonalityAction::ExitProcess(_) => {
                        if contexts[active].tid != LAUNCHER_TID {
                            return Ok(());
                        }
                        return Err(
                            "wc3: ExitProcess action is not wired into the launcher loop".into(),
                        );
                    }
                };
                if let Some((frame, input)) =
                    draw_text_input.filter(|(frame, _)| frame[5] == 0x0000_0411)
                {
                    let output = read_guest_words(&memory, frame[4], 4)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 DrawTextA CALCRECT hdc=0x{:08x} count={} format=0x{:08x} input=[{},{},{},{}] output=[{},{},{},{}] height={}",
                            frame[1],
                            frame[3],
                            frame[5],
                            input[0] as i32,
                            input[1] as i32,
                            input[2] as i32,
                            input[3] as i32,
                            output[0] as i32,
                            output[1] as i32,
                            output[2] as i32,
                            output[3] as i32,
                            output[3].wrapping_sub(output[1])
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 DrawTextA placement client=500x400 measured={}x{} expected_final_rect=[47,376,453,392]",
                            output[2].wrapping_sub(output[0]),
                            output[3].wrapping_sub(output[1])
                        ),
                    );
                }
                if let Some(call) = callback {
                    let callback_esp = exit
                        .registers
                        .esp
                        .checked_sub(20)
                        .ok_or("callback stack underflow")?;
                    for (offset, value) in [
                        (0, thunk32::GUEST_RETURN_ADDRESS),
                        (4, call.arguments[0]),
                        (8, call.arguments[1]),
                        (12, call.arguments[2]),
                        (16, call.arguments[3]),
                    ] {
                        memory
                            .write(callback_esp + offset, &value.to_le_bytes())
                            .map_err(str::to_owned)?;
                    }
                    contexts[active].continuation = Some(GuestContinuation {
                        import_resume_eip: exit.registers.eip,
                        import_esp: exit.registers.esp,
                        completion_eax: call.completion_eax,
                        wndproc: call.address,
                        hwnd: call.arguments[0],
                        message: call.arguments[1],
                    });
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CALL_GUEST tid={} reason=UpdateWindow/WM_PAINT hwnd=0x{:08x} wndproc=0x{:08x} message=0x{:08x} wparam=0x{:08x} lparam=0x{:08x}",
                            contexts[active].tid,
                            call.arguments[0],
                            call.address,
                            call.arguments[1],
                            call.arguments[2],
                            call.arguments[3]
                        ),
                    );
                    if let Some(window) = session.windows.get(&call.arguments[0]) {
                        if call.arguments[0] == WINDOW_HANDLE_BASE && call.arguments[1] == 0x000f {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 UI4 ROOT WM_PAINT ENTER hwnd=0x{:08x}",
                                    call.arguments[0]
                                ),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 UI4 PAINT BEGIN hwnd=0x{:08x}", call.arguments[0]),
                        );
                        let _ = window;
                    }
                    let mut registers = exit.registers;
                    registers.eip = call.address;
                    registers.esp = callback_esp;
                    contexts[active]
                        .context
                        .set_registers(registers)
                        .map_err(|error| error.to_string())?;
                    continue;
                }
                let mut registers = exit.registers;
                registers.eax = result;
                contexts[active]
                    .context
                    .set_registers(registers)
                    .map_err(|error| error.to_string())?;
                if import.symbol != "PeekMessageA" {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "wc3: return #{} {}!{} eax=0x{result:08x}",
                            session.launcher().xp.call_count,
                            import.module,
                            import.symbol,
                        ),
                    );
                }
                if let Some(thread) = session.absorb_runnable_thread() {
                    contexts.push(create_thread_context(&address_space, &thread)?);
                }
                if let Some(request) = session.take_window_presentation() {
                    present_window(request, &mut frames, window_rgba, &session)?;
                }
                if contexts[active].tid == LAUNCHER_TID {
                    active = 0;
                }
            }
            ExitKind::Exception if active_key.pid != LAUNCHER_PID => {
                let child = pending_child
                    .as_ref()
                    .filter(|child| child.pid == active_key.pid && child.tid == active_key.tid)
                    .ok_or_else(|| "exception child missing pending state".to_owned())?;
                let scope = child_execution_scope(child).map_err(str::to_owned)?;
                let exception = decode_child_exception(exit.detail, exit.qualification);
                let registers = exit.registers;
                let quiet_exception = quiet_war3_exception(exception, registers)
                    || current_child_seh_handler(child, registers.fs_base)
                        .is_some_and(|handler| boring_war3_single_step(exception, registers, handler));
                if !quiet_exception
                    && exception.vector == Some(1)
                    && matches!(registers.eip, 0x0045_af54 | 0x0045_af5a)
                {
                    let gate = child_read_u32(child, 0x0049_a430);
                    let reset = child_read_u32(child, 0x0049_2614);
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD DB STATUS eip=0x{:08x} dr6={:?} gate_49a430={:?} reset_492614={:?}",
                            registers.eip,
                            exception.debug_status,
                            gate,
                            reset,
                        ),
                    );
                }
                if !quiet_exception {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION RAW detail=0x{:08x} qualification=0x{:016x}",
                        exit.detail, exit.qualification,
                    ),
                );
                // Preceding bytes expose the call/return or pointer-producing
                // instruction; these are raw bytes, not instruction boundaries.
                for distance in [32u32, 16] {
                    if let Some(start) = registers.eip.checked_sub(distance) {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD EXCEPTION CODE BEFORE address=0x{:08x} bytes=\"{}\"",
                                start,
                                exception_code_window(&child.address_space, start),
                            ),
                        );
                    }
                }
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} vector={} name=\"{}\" type={} valid={} error_valid={} error={}",
                        active_key.pid,
                        active_key.tid,
                        scope,
                        registers.eip,
                        registers.esp,
                        exception
                            .vector
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".into()),
                        exception.name,
                        exception
                            .interruption_type
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".into()),
                        exception.valid as u8,
                        exception
                            .error_valid
                            .map(|value| (value as u8).to_string())
                            .unwrap_or_else(|| "-".into()),
                        exception
                            .error
                            .map(|value| format!("0x{value:08x}"))
                            .unwrap_or_else(|| "-".into()),
                    ),
                );
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION FAULT {}",
                        child_exception_fault_detail(exception),
                    ),
                );
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION REGS eax=0x{:08x} ebx=0x{:08x} ecx=0x{:08x} edx=0x{:08x} esi=0x{:08x} edi=0x{:08x} ebp=0x{:08x} esp=0x{:08x} eip=0x{:08x} eflags=0x{:08x} fs_base=0x{:08x}",
                        registers.eax,
                        registers.ebx,
                        registers.ecx,
                        registers.edx,
                        registers.esi,
                        registers.edi,
                        registers.ebp,
                        registers.esp,
                        registers.eip,
                        registers.eflags,
                        registers.fs_base,
                    ),
                );
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION CODE eip=0x{:08x} bytes=\"{}\"",
                        registers.eip,
                        exception_code_window(&child.address_space, registers.eip),
                    ),
                );
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EXCEPTION STACK esp=0x{:08x} words=[redacted]",
                        registers.esp,
                    ),
                );
                if let ChildExecutionState::DllInitRunning { native_index } = child.execution {
                    let module = child
                        .native_modules
                        .get(native_index)
                        .ok_or_else(|| "child DLL exception native index".to_owned())?;
                    if module.stored.eq_ignore_ascii_case("Storm.dll")
                        && registers.eip == 0x1503_62ee
                    {
                        log_child_fault_precursor(
                            child,
                            module,
                            session
                                .process(active_key.pid)
                                .ok_or_else(|| "faulted child process missing".to_owned())?,
                            active_key.pid,
                            active_key.tid,
                            registers.eax,
                        )?;
                    }
                }
                }
                let null_execute = exception.vector == Some(14)
                    && registers.eip == 0
                    && exception.fault_linear == Some(0)
                    && exception.error.is_some_and(|error| error & 0x10 != 0);
                let null_loop_signature = null_execute.then(|| {
                    log_null_call_diagnostic(child, active_key.pid, active_key.tid, registers)
                }).flatten();
                let child = pending_child
                    .as_mut()
                    .filter(|child| child.pid == active_key.pid && child.tid == active_key.tid)
                    .ok_or_else(|| "exception child missing mutable pending state".to_owned())?;
                if null_execute {
                    if let Some(signature) = null_loop_signature {
                        let count = match child.repeated_null_call {
                            Some(mut watch) if watch.signature == signature => {
                                watch.count = watch.count.saturating_add(1);
                                child.repeated_null_call = Some(watch);
                                watch.count
                            }
                            _ => {
                                child.repeated_null_call = Some(NullLoopWatch {
                                    signature,
                                    count: 1,
                                });
                                1
                            }
                        };
                        if count >= 3 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD OPERATOR STOP reason=repeated-null-call pid={} tid={} return=0x{:08x} slot=0x{:08x} target=0x00000000 repeats={}",
                                    signature.pid,
                                    signature.tid,
                                    signature.return_address,
                                    signature.slot,
                                    count,
                                ),
                            );
                            return Ok(());
                        }
                    } else {
                        child.repeated_null_call = None;
                    }
                }
                if exception.vector == Some(0) {
                    let Some(progress) = divide_loop_progress(child) else {
                        child.repeated_divide_fault = None;
                        begin_child_seh_dispatch(child, &mut contexts[active], exception, registers)?;
                        continue;
                    };
                    let signature = DivideLoopSignature {
                        pid: active_key.pid,
                        tid: active_key.tid,
                        eip: registers.eip,
                        esp: registers.esp,
                        eax: registers.eax,
                        ecx: registers.ecx,
                        edx: registers.edx,
                        progress,
                    };
                    let count = match child.repeated_divide_fault {
                        Some(mut watch) if watch.signature == signature => {
                            watch.count = watch.count.saturating_add(1);
                            child.repeated_divide_fault = Some(watch);
                            watch.count
                        }
                        _ => {
                            child.repeated_divide_fault = Some(DivideLoopWatch {
                                signature,
                                count: 1,
                            });
                            1
                        }
                    };
                    if count >= 3 {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD OPERATOR STOP reason=repeated-divide-error pid={} tid={} eip=0x{:08x} esp=0x{:08x} eax=0x{:08x} ecx=0x{:08x} edx=0x{:08x} table_base=0x{:08x} index=0x{:08x} state_ptr=0x{:08x} input=0x{:02x} accum=0x{:04x} repeats={}",
                                signature.pid,
                                signature.tid,
                                signature.eip,
                                signature.esp,
                                signature.eax,
                                signature.ecx,
                                signature.edx,
                                signature.progress.table_base,
                                signature.progress.index,
                                signature.progress.state_ptr,
                                signature.progress.input,
                                signature.progress.accumulator,
                                count,
                            ),
                        );
                        return Ok(());
                    }
                } else {
                    child.repeated_divide_fault = None;
                }
                begin_child_seh_dispatch(child, &mut contexts[active], exception, registers)?;
                continue;
            }
            ExitKind::Halted => {
                if active_key.pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_ref()
                        .filter(|child| child.pid == active_key.pid && child.tid == active_key.tid)
                        .ok_or_else(|| "halted child missing pending state".to_owned())?;
                    let scope = child_execution_scope(child).map_err(str::to_owned)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXECUTION FAULT pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} kind=Halted detail={}",
                            active_key.pid,
                            active_key.tid,
                            scope,
                            exit.registers.eip,
                            exit.registers.esp,
                            exit.detail
                        ),
                    );
                    return Ok(());
                }
                let halted_tid = contexts.remove(active).tid;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3: x86 context halted tid={halted_tid} after {} calls",
                        session.launcher().xp.call_count
                    ),
                );
                if contexts.is_empty() {
                    return Ok(());
                }
                active %= contexts.len();
            }
            kind => {
                if active_key.pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_ref()
                        .filter(|child| child.pid == active_key.pid && child.tid == active_key.tid)
                        .ok_or_else(|| "faulted child missing pending state".to_owned())?;
                    let scope = child_execution_scope(child).map_err(str::to_owned)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXECUTION FAULT pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} kind={:?} detail={}",
                            active_key.pid,
                            active_key.tid,
                            scope,
                            exit.registers.eip,
                            exit.registers.esp,
                            kind,
                            exit.detail
                        ),
                    );
                    return Ok(());
                }
                return Err(format!(
                    "x86 context stopped: kind={kind:?} detail={} qualification=0x{:x} eip=0x{:08x}",
                    exit.detail, exit.qualification, exit.registers.eip,
                ));
            }
        }
    }
}
