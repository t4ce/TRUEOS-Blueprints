//! On-demand observation at coordinator stops; never changes guest state.
use super::*;
use wc3::debug_command::{Command, Lines};

pub struct DebugShell {
    lines: Lines,
    next_poll: std::time::Instant,
    exits: u8,
}
impl DebugShell {
    pub fn new() -> Self {
        Self {
            lines: Lines::default(),
            next_poll: std::time::Instant::now(),
            exits: 0,
        }
    }
    pub fn poll(
        &mut self,
        contexts: &[GuestContext],
        child: Option<&PendingChild>,
        launcher: &AddressSpace,
        session: &Wc3Session,
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
                    execute(command, contexts, child, launcher, session, active)
                });
                if let Err(error) = result {
                    logl::log(level::IMPORTANT, format_args!("WC3 DEBUG ERROR {error}"));
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
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 DEBUG STACK CANDIDATES pid={pid} address=0x{:08x} words={words:08x?}",
                    address + (i * 16) as u32
                ),
            );
        } else {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 DEBUG MEMORY pid={pid} address=0x{:08x} bytes={chunk:02x?}",
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
    session: &Wc3Session,
    active: ThreadKey,
) -> Result<(), String> {
    match command {
        Command::Help => logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 DEBUG HELP: debug state | debug regs PID TID | debug mem PID ADDRESS BYTES(1..256) | debug stack PID TID WORDS(1..64) | debug object PID HANDLE; numbers decimal or 0xhex; read-only stopped-state snapshots"
            ),
        ),
        Command::State => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 DEBUG STATE active={active:?} contexts={} runnable={} blocked={} iocp={} critical_waiters={}",
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
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 DEBUG WAIT pid={} tid={} count={} handle0=0x{:08x} timeout={} wait_all={}",
                            c.pid, c.tid, wait.count, wait.handles[0], wait.timeout, wait.wait_all
                        ),
                    );
                }
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 DEBUG THREAD pid={} tid={} started={} exit_code={:?} eip=0x{:08x} esp=0x{:08x} snapshot=last-stopped",
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
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 DEBUG PROCESS pid={pid} exit_code={:?} queued_messages={}",
                        process.exit_code,
                        process.xp.pending_message_count()
                    ),
                );
            }
            for (hwnd, w) in session.windows.iter().take(32) {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 DEBUG WINDOW hwnd=0x{hwnd:08x} owner={:?} wndproc=0x{:08x} user_data=0x{:08x} param=0x{:08x} paint_pending={}",
                        w.owner, w.wndproc, w.user_data, w.param, w.paint_pending
                    ),
                );
            }
        }
        Command::Registers { pid, tid } => {
            let r = registers(contexts, pid, tid)?;
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 DEBUG REGS pid={pid} tid={tid} snapshot=last-stopped eip={:08x} esp={:08x} ebp={:08x} eax={:08x} ebx={:08x} ecx={:08x} edx={:08x} esi={:08x} edi={:08x} eflags={:08x} fs_base={:08x}",
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
        Command::Object { pid, handle } => logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 DEBUG OBJECT pid={pid} handle=0x{handle:08x} {}",
                session.describe_handle(pid, handle)
            ),
        ),
    }
    Ok(())
}
