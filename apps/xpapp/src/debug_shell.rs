//! Observation and explicitly requested notification experiments at coordinator stops.
use super::*;
use xpapp::debug_command::{Command, Lines};

pub struct DebugShell {
    lines: Lines,
    next_poll: std::time::Instant,
    exits: u8,
    perf_sample: Option<(std::time::Instant, u64, u64, u64)>,
}
impl DebugShell {
    pub fn new() -> Self {
        Self {
            lines: Lines::default(),
            next_poll: std::time::Instant::now(),
            exits: 0,
            perf_sample: None,
        }
    }
    pub fn poll(
        &mut self,
        contexts: &[GuestContext],
        child: Option<&PendingChild>,
        launcher: &AddressSpace,
        session: &mut XpappSession,
        active: ThreadKey,
        preempted: bool,
    ) {
        // Do not add a host clock/input crossing on every guest API call.
        self.exits = self.exits.wrapping_add(1);
        if !preempted && self.exits % 32 != 1 {
            return;
        }
        let now = std::time::Instant::now();
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + std::time::Duration::from_millis(100);
        let mut bytes = [0; 192];
        let count = trueos::vshell::attached_read_available(&mut bytes);
        for byte in &bytes[..count] {
            if let Some(command) = self.lines.push(*byte) {
                let result = command.map_err(str::to_owned).and_then(|command| {
                    if command == Command::Perf {
                        let (draws, swaps) = session.processes.values().map(|p| p.xp.gl_progress())
                            .fold((0u64, 0u64), |(d, s), (dd, ss)| (d.saturating_add(dd), s.saturating_add(ss)));
                        let exits = GuestThreadContext::last_execution_diagnostic().sequence;
                        let now = std::time::Instant::now();
                        if let Some((before, old_draws, old_swaps, old_exits)) = self.perf_sample {
                            let elapsed = now.duration_since(before).as_secs_f64();
                            logl::emit(level::IMPORTANT, format_args!(
                                "XPAPP PERF interval_s={elapsed:.3} draws={} guest_swaps={} guest_swaps_per_s={:.3} execution_exits={} transport={}",
                                draws.saturating_sub(old_draws), swaps.saturating_sub(old_swaps),
                                swaps.saturating_sub(old_swaps) as f64 / elapsed.max(0.000001),
                                exits.saturating_sub(old_exits), if cfg!(feature = "actor-execution") { "actor" } else { "direct" },
                            ));
                        } else {
                            logl::emit(level::IMPORTANT, format_args!("XPAPP PERF baseline draws={draws} guest_swaps={swaps} execution_exits={exits}; repeat debug perf for interval"));
                        }
                        self.perf_sample = Some((now, draws, swaps, exits));
                        Ok(())
                    } else {
                        execute(command, contexts, child, launcher, session, active)
                    }
                });
                if let Err(error) = result {
                    logl::emit(level::IMPORTANT, format_args!("XPAPP DEBUG ERROR {error}"));
                }
            }
        }
    }
}
fn space<'a>(
    pid: u32,
    child: Option<&'a PendingChild>,
    launcher: &'a AddressSpace,
) -> Result<&'a AddressSpace, String> {
    if pid == LAUNCHER_PID {
        return Ok(launcher);
    }
    child
        .filter(|c| c.pid == pid)
        .map(|c| &c.address_space)
        .ok_or("unknown live address space".into())
}
fn registers(contexts: &[GuestContext], pid: u32, tid: u32) -> Result<Registers, String> {
    contexts
        .iter()
        .find(|c| c.pid == pid && c.tid == tid)
        .ok_or("unknown live thread")?
        .context
        .registers()
}
fn dump(
    space: &AddressSpace,
    pid: u32,
    address: u32,
    count: usize,
    stack: bool,
) -> Result<(), String> {
    let mut bytes = vec![0; count];
    let read = space
        .read(address, &mut bytes)
        .map_err(|error| error.to_string())?;
    if read != count {
        return Err(format!(
            "short guest read address=0x{address:08x} requested={count} read={read}"
        ));
    }
    for (i, chunk) in bytes.chunks(16).enumerate() {
        if stack {
            let words: Vec<_> = chunk
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG STACK CANDIDATES pid={pid} address=0x{:08x} words={words:08x?}",
                    address + (i * 16) as u32
                ),
            );
        } else {
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG MEMORY pid={pid} address=0x{:08x} bytes={chunk:02x?}",
                    address + (i * 16) as u32
                ),
            );
        }
    }
    Ok(())
}
fn execute(
    command: Command,
    contexts: &[GuestContext],
    child: Option<&PendingChild>,
    launcher: &AddressSpace,
    session: &mut XpappSession,
    active: ThreadKey,
) -> Result<(), String> {
    match command {
        Command::Perf => return Err("performance sampling requires the live shell sampler".into()),
        Command::Isolate { pid, tid, texture } => {
            session
                .process_mut(pid)
                .ok_or("unknown process")?
                .xp
                .debug_isolate_gl_texture(tid, texture)?;
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG ISOLATE pid={pid} tid={tid} texture={texture:?} background=gray guest_execution=unchanged"
                ),
            );
        }
        Command::Help => logl::emit(
            level::IMPORTANT,
            format_args!(
                "XPAPP DEBUG HELP: debug depth PID TID TEXTURE bypass / debug depth PID TID restore | debug draws PID TID COUNT(0..256) | debug perf | debug state | debug regs PID TID | debug mem PID ADDRESS BYTES(1..256) | debug stack PID TID WORDS(1..64) | debug object PID HANDLE | debug post PID HWND MESSAGE WPARAM LPARAM (explicit queued notification experiment); numbers decimal or 0xhex"
            ),
        ),
        Command::Depth { pid, tid, texture } => {
            session
                .process_mut(pid)
                .ok_or("unknown process")?
                .xp
                .debug_texture_depth(tid, texture)?;
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG DEPTH pid={pid} tid={tid} bypass_texture={texture:?} experiment=true guest_state=unchanged"
                ),
            );
        }
        Command::Draws { pid, tid, count } => {
            session
                .process_mut(pid)
                .ok_or("unknown process")?
                .xp
                .debug_capture_gl_draws(tid, count)?;
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG DRAWS pid={pid} tid={tid} remaining={count} capture=next-draws rendering=unchanged"
                ),
            );
        }
        Command::Post {
            pid,
            hwnd,
            message,
            wparam,
            lparam,
        } => {
            let window = session.windows.get(&hwnd).ok_or("unknown live window")?;
            if window.owner.pid != pid {
                return Err("window belongs to a different process".into());
            }
            let owner = window.owner;
            let process = session.process_mut(pid).ok_or("unknown process")?;
            if process.exit_code.is_some() {
                return Err("process has exited".into());
            }
            process
                .xp
                .debug_post_window_message(hwnd, message, wparam, lparam)?;
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG POST QUEUED pid={pid} tid={} hwnd=0x{hwnd:08x} message=0x{message:04x} wparam=0x{wparam:08x} lparam=0x{lparam:08x} delivery=guest-message-pump experiment=true",
                    owner.tid
                ),
            );
        }
        Command::State => {
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG STATE active={active:?} contexts={} runnable={} blocked={} iocp={} critical_waiters={}",
                    contexts.len(),
                    session.runnable.len(),
                    session.blocked.len(),
                    session.iocp_waiters.len(),
                    session.critical_waiters.len()
                ),
            );
            for c in contexts.iter().take(64) {
                let r = c.context.registers()?;
                if let Some(wait) = session.blocked.get(&c.key()) {
                    logl::emit(
                        level::IMPORTANT,
                        format_args!(
                            "XPAPP DEBUG WAIT pid={} tid={} count={} handle0=0x{:08x} timeout={} wait_all={}",
                            c.pid, c.tid, wait.count, wait.handles[0], wait.timeout, wait.wait_all
                        ),
                    );
                }
                logl::emit(
                    level::IMPORTANT,
                    format_args!(
                        "XPAPP DEBUG THREAD pid={} tid={} started={} exit_code={:?} eip=0x{:08x} esp=0x{:08x} snapshot=last-stopped",
                        c.pid,
                        c.tid,
                        c.started,
                        session.thread_exit_code(c.key()),
                        r.eip,
                        r.esp
                    ),
                );
            }
            for (pid, process) in session.processes.iter().take(32) {
                logl::emit(
                    level::IMPORTANT,
                    format_args!(
                        "XPAPP DEBUG PROCESS pid={pid} exit_code={:?} queued_messages={}",
                        process.exit_code,
                        process.xp.pending_message_count()
                    ),
                );
            }
            for (hwnd, w) in session.windows.iter().take(32) {
                logl::emit(
                    level::IMPORTANT,
                    format_args!(
                        "XPAPP DEBUG WINDOW hwnd=0x{hwnd:08x} owner={:?} wndproc=0x{:08x} user_data=0x{:08x} param=0x{:08x} paint_pending={}",
                        w.owner, w.wndproc, w.user_data, w.param, w.paint_pending
                    ),
                );
            }
        }
        Command::Registers { pid, tid } => {
            let r = registers(contexts, pid, tid)?;
            logl::emit(
                level::IMPORTANT,
                format_args!(
                    "XPAPP DEBUG REGS pid={pid} tid={tid} snapshot=last-stopped eip={:08x} esp={:08x} ebp={:08x} eax={:08x} ebx={:08x} ecx={:08x} edx={:08x} esi={:08x} edi={:08x} eflags={:08x} fs_base={:08x}",
                    r.eip,
                    r.esp,
                    r.ebp,
                    r.eax,
                    r.ebx,
                    r.ecx,
                    r.edx,
                    r.esi,
                    r.edi,
                    r.eflags,
                    r.fs_base
                ),
            );
        }
        Command::Memory {
            pid,
            address,
            bytes,
        } => dump(space(pid, child, launcher)?, pid, address, bytes, false)?,
        Command::Stack { pid, tid, words } => {
            let r = registers(contexts, pid, tid)?;
            let space = space(pid, child, launcher)?;
            let mut bounds = [0; 8];
            let tib_bounds = r.fs_base.checked_add(4).ok_or("TIB overflow")?;
            if space
                .read(tib_bounds, &mut bounds)
                .map_err(|e| e.to_string())?
                != 8
            {
                return Err("short TIB bounds read".into());
            }
            let top = u32::from_le_bytes(bounds[..4].try_into().unwrap());
            let limit = u32::from_le_bytes(bounds[4..].try_into().unwrap());
            let end = r
                .esp
                .checked_add((words * 4) as u32)
                .ok_or("stack overflow")?;
            if limit >= top || r.esp < limit || end > top {
                return Err("requested stack sample outside guest TIB bounds".into());
            }
            dump(space, pid, r.esp, words * 4, true)?;
        }
        Command::Object { pid, handle } => logl::emit(
            level::IMPORTANT,
            format_args!(
                "XPAPP DEBUG OBJECT pid={pid} handle=0x{handle:08x} {}",
                session.describe_handle(pid, handle)
            ),
        ),
    }
    Ok(())
}
