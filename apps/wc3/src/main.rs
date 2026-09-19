use sha2::{Digest, Sha256};
use trueos::{
    async_fs,
    logl::{self, level},
    ui4_scene::{self, Damage, Frame, rgba},
    x86::{AddressSpace, Context, ExitKind, Permissions, Registers},
};
use wc3::{
    EXPECTED_SHA256, LAUNCHER_PATH, pe32,
    process::{DispatchResult, Frontier, GuestMemory, PreparedProcess, STACK_BASE, STACK_BYTES, ThreadObject, WindowRequest},
    thunk32,
};

fn main() {
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("wc3: Tokio runtime failed: {error}"),
            );
            return;
        }
    };
    if let Err(error) = runtime.block_on(run()) {
        logl::log(level::ERROR, format_args!("wc3: {error}"));
    }
}

async fn run() -> Result<(), String> {
    let bytes = async_fs::read_file(LAUNCHER_PATH.as_bytes())
        .await
        .map_err(|error| format!("read {LAUNCHER_PATH}: TRUEOSFS error {error}"))?;
    if Sha256::digest(&bytes).as_slice() != EXPECTED_SHA256 {
        return Err("launcher SHA-256 does not match Warcraft III RoC 1.00".into());
    }
    let materialized = pe32::materialize(&bytes).map_err(str::to_owned)?;
    let imports = materialized.imports.len();
    let mut prepared = PreparedProcess::new(materialized).map_err(str::to_owned)?;
    let (desktop_width, desktop_height) = ui4_scene::output_dimensions()
        .map_err(|error| format!("query UI4 output dimensions: {error:?}"))?;
    prepared.xp.set_desktop_size(desktop_width, desktop_height);
    let address_space = AddressSpace::create().map_err(|error| error.to_string())?;
    for mapping in &prepared.mappings {
        let mut permissions = Permissions::READ | Permissions::WRITE;
        if mapping.executable {
            permissions |= Permissions::EXECUTE;
        }
        address_space
            .map(mapping.address, mapping.bytes.len(), permissions)
            .map_err(|error| format!("map 0x{:08x}: {error}", mapping.address))?;
        let written = address_space
            .write(mapping.address, &mapping.bytes)
            .map_err(|error| format!("write 0x{:08x}: {error}", mapping.address))?;
        if written != mapping.bytes.len() {
            return Err("short x86 mapping write".into());
        }
    }

    let esp = STACK_BASE + u32::try_from(STACK_BYTES).map_err(|_| "stack too large")? - 16;
    let registers = Registers {
        esp,
        eip: pe32::IMAGE_BASE + pe32::ENTRY_RVA,
        eflags: 0x202,
        fs_base: wc3::process::TEB_VA,
        ..Registers::default()
    };
    let context = Context::create(&address_space, registers).map_err(|error| error.to_string())?;
    let mut memory = X86Memory(&address_space);
    logl::log(
        level::INFO,
        format_args!(
            "wc3: launcher accepted imports={imports} entry=0x{:08x}; execution owned by Blueprint",
            registers.eip,
        ),
    );

    let mut contexts = vec![GuestContext {
        tid: 1,
        context,
        started: false,
    }];
    let mut window_frame: Option<Frame> = None;
    let mut active = 0usize;
    loop {
        prepared.xp.set_current_thread(contexts[active].tid);
        let exit = if contexts[active].started {
            contexts[active].context.resume().await
        } else {
            contexts[active].started = true;
            contexts[active].context.run().await
        }
        .map_err(|error| error.to_string())?;
        match exit.kind {
            ExitKind::VmCall => {
                if exit.registers.eip == thunk32::THREAD_EXIT_AFTER_VMCALL {
                    let exited = contexts.remove(active);
                    prepared
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
                let import_id = exit.registers.eax;
                let import = prepared
                    .xp
                    .import(import_id)
                    .cloned()
                    .ok_or_else(|| format!("unknown import trap id={import_id}"))?;
                let call_number = prepared.xp.call_count + 1;
                logl::log(
                    level::INFO,
                    format_args!("wc3: call #{call_number} {}!{}", import.module, import.symbol),
                );
                let result = prepared
                    .xp
                    .dispatch(import_id, exit.registers.esp, &mut memory)
                    .map_err(|error| {
                        format!(
                            "call #{} {}!{}: {error}",
                            prepared.xp.call_count, import.module, import.symbol
                        )
                    })?;
                let result = match result {
                    DispatchResult::Value(value) => value,
                    DispatchResult::Frontier(Frontier::CreateProcessA(frame)) => {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BLUEPRINT FRONTIER: CreateProcessA call #{} ret=0x{:08x} command_line=0x{:08x} startup=0x{:08x} process_info=0x{:08x}",
                                prepared.xp.call_count,
                                frame.return_address,
                                frame.command_line,
                                frame.startup_info,
                                frame.process_information,
                            ),
                        );
                        logl::log(
                            level::INFO,
                            format_args!(
                                "wc3: frontier state deferred_tid={:?} focused_root={:?}",
                                prepared.xp.deferred_runnable_tid(),
                                prepared.xp.focused_window(),
                            ),
                        );
                        return Ok(());
                    }
                };
                let mut registers = exit.registers;
                registers.eax = result;
                contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3: return #{} {}!{} eax=0x{result:08x}",
                        prepared.xp.call_count, import.module, import.symbol,
                    ),
                );
                if let Some(request) = prepared.xp.take_window_request() {
                    present_window(request, &mut window_frame)?;
                }
                // The proven launcher resumes TID2 but does not execute it
                // before the main thread reaches CreateProcessA (#89).
                // Keep the runnable state in the personality; scheduling it
                // is deliberately outside this migration checkpoint.
                active = 0;
            }
            ExitKind::Halted => {
                let halted_tid = contexts.remove(active).tid;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3: x86 context halted tid={halted_tid} after {} calls",
                        prepared.xp.call_count
                    ),
                );
                if contexts.is_empty() {
                    return Ok(());
                }
                active %= contexts.len();
            }
            kind => {
                return Err(format!(
                    "x86 context stopped: kind={kind:?} detail={} qualification=0x{:x} eip=0x{:08x}",
                    exit.detail, exit.qualification, exit.registers.eip,
                ));
            }
        }
    }
}

fn present_window(request: WindowRequest, frame: &mut Option<Frame>) -> Result<(), String> {
    match request {
        WindowRequest::Show {
            x,
            y,
            width,
            height,
        } => {
            if frame.is_some() {
                return Ok(());
            }
            let mut opened = Frame::open(x, y, width, height)
                .map_err(|error| format!("create WC3 UI4 window: {error:?}"))?;
            opened
                .begin(rgba(0, 0, 0, 255))
                .and_then(|()| opened.publish(Damage::full(width, height)))
                .map_err(|error| format!("publish WC3 UI4 window: {error:?}"))?;
            *frame = Some(opened);
        }
        WindowRequest::Hide => {
            *frame = None;
        }
    }
    Ok(())
}

struct GuestContext {
    tid: u32,
    context: Context,
    started: bool,
}

fn create_thread_context(
    address_space: &AddressSpace,
    thread: &ThreadObject,
) -> Result<GuestContext, String> {
    // Each logical thread gets private x86 stack state. The context remains a
    // logical Blueprint object; every run/resume may use a different carrier.
    let stack_bytes = usize::try_from(thread.requested_stack_size.max(0x1000))
        .map_err(|_| "thread stack size")?;
    let stack_bytes = stack_bytes.next_multiple_of(0x1000);
    let stack_top = STACK_BASE
        .checked_sub(
            thread
                .tid
                .checked_mul(0x0010_0000)
                .ok_or("thread stack index")?,
        )
        .ok_or("thread stack address")?;
    let stack_base = stack_top
        .checked_sub(u32::try_from(stack_bytes).map_err(|_| "thread stack range")?)
        .ok_or("thread stack address")?;
    let teb = thread_teb_va(thread.tid)?;
    address_space
        .map(teb, 0x1000, Permissions::READ | Permissions::WRITE)
        .map_err(|error| error.to_string())?;
    let initialized_exception_list = u32::MAX.to_le_bytes();
    if address_space
        .write(teb, &initialized_exception_list)
        .map_err(|error| error.to_string())?
        != initialized_exception_list.len()
    {
        return Err("short x86 TEB write".into());
    }
    address_space
        .map(
            stack_base,
            stack_bytes,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| error.to_string())?;
    let esp = stack_top - 8;
    let mut entry_frame = [0; 8];
    entry_frame[..4].copy_from_slice(&thunk32::THREAD_EXIT_ADDRESS.to_le_bytes());
    entry_frame[4..].copy_from_slice(&thread.parameter.to_le_bytes());
    if address_space
        .write(esp, &entry_frame)
        .map_err(|error| error.to_string())?
        != entry_frame.len()
    {
        return Err("short x86 thread stack write".into());
    }
    let registers = Registers {
        esp,
        eip: thread.start_address,
        eflags: 0x202,
        fs_base: teb,
        ..Registers::default()
    };
    let context = Context::create(address_space, registers).map_err(|error| error.to_string())?;
    Ok(GuestContext {
        tid: thread.tid,
        context,
        started: false,
    })
}

fn thread_teb_va(tid: u32) -> Result<u32, String> {
    wc3::process::TEB_VA
        .checked_add(
            tid.checked_sub(1)
                .ok_or_else(|| "invalid x86 thread id".to_owned())?
                .checked_mul(0x1000)
                .ok_or_else(|| "x86 TEB index overflow".to_owned())?,
        )
        .filter(|address| *address < wc3::process::HEAP_VA)
        .ok_or_else(|| "x86 TEB address space exhausted".to_owned())
}

struct X86Memory<'a>(&'a AddressSpace);

impl GuestMemory for X86Memory<'_> {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
        match self.0.read(address, output) {
            Ok(read) if read == output.len() => Ok(()),
            _ => Err("x86 guest read"),
        }
    }

    fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
        match self.0.write(address, input) {
            Ok(written) if written == input.len() => Ok(()),
            _ => Err("x86 guest write"),
        }
    }
}
