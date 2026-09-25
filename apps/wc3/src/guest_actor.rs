//! A Windows thread's x86 context is owned by one Tokio task. The coordinator
//! keeps only stopped-state snapshots and grants execution one exit at a time.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use tokio::sync::{mpsc, oneshot};
use trueos::x86::{Context, DebugRegisters, Exit, ExtendedState, Registers};

#[derive(Clone, Copy, Debug)]
pub enum ExecutionStage {
    Idle,
    Submit,
    Accept,
    Enter,
    Exit,
    Reply,
    Receive,
}

#[derive(Clone, Copy, Debug)]
pub struct ExecutionDiagnostic {
    pub sequence: u64,
    pub stage: ExecutionStage,
    pub pid: u32,
    pub tid: u32,
}

static NEXT_EXECUTION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static LAST_EXECUTION: Mutex<ExecutionDiagnostic> = Mutex::new(ExecutionDiagnostic {
    sequence: 0,
    stage: ExecutionStage::Idle,
    pid: 0,
    tid: 0,
});

fn record_execution(sequence: u64, stage: ExecutionStage, pid: u32, tid: u32) {
    *LAST_EXECUTION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = ExecutionDiagnostic {
        sequence,
        stage,
        pid,
        tid,
    };
}

#[derive(Clone, Debug)]
struct ExecutionProvenance {
    provider: String,
    caller_ret: u32,
}

enum ThreadCommand {
    Execute {
        sequence: u64,
        provenance: Option<ExecutionProvenance>,
        registers: Option<Registers>,
        debug_registers: Option<DebugRegisters>,
        extended_state: Option<ExtendedState>,
        reply: oneshot::Sender<Result<ThreadEvent, String>>,
    },
}

struct ThreadEvent {
    exit: Exit,
    debug_registers: DebugRegisters,
    extended_state: ExtendedState,
}

/// Coordinator-side handle. Synchronous access is to the last stopped state;
/// writes are bundled into the next execution permit.
pub struct GuestThreadContext {
    commands: mpsc::Sender<ThreadCommand>,
    _task: tokio::task::JoinHandle<()>,
    pid: u32,
    tid: u32,
    registers: Registers,
    debug_registers: DebugRegisters,
    extended_state: ExtendedState,
    registers_dirty: bool,
    debug_dirty: bool,
    extended_dirty: bool,
    next_execution_provenance: Option<ExecutionProvenance>,
}

impl GuestThreadContext {
    pub fn spawn(context: Context, pid: u32, tid: u32) -> Result<Self, String> {
        let registers = context.registers().map_err(|error| error.to_string())?;
        let debug_registers = context
            .debug_registers()
            .map_err(|error| error.to_string())?;
        let extended_state = context
            .extended_state()
            .map_err(|error| error.to_string())?;
        let (commands, receiver) = mpsc::channel(1);
        let task = tokio::spawn(guest_thread_task(context, receiver, pid, tid));
        Ok(Self {
            commands,
            _task: task,
            pid,
            tid,
            registers,
            debug_registers,
            extended_state,
            registers_dirty: false,
            debug_dirty: false,
            extended_dirty: false,
            next_execution_provenance: None,
        })
    }

    pub fn last_execution_diagnostic() -> ExecutionDiagnostic {
        *LAST_EXECUTION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_execution_provenance(&mut self, provider: String, caller_ret: u32) {
        self.next_execution_provenance = Some(ExecutionProvenance {
            provider,
            caller_ret,
        });
    }

    pub fn registers(&self) -> Result<Registers, String> {
        Ok(self.registers)
    }

    pub fn set_registers(&mut self, registers: Registers) -> Result<(), String> {
        self.registers = registers;
        self.registers_dirty = true;
        Ok(())
    }

    pub fn debug_registers(&self) -> Result<DebugRegisters, String> {
        Ok(self.debug_registers)
    }

    pub fn set_debug_registers(&mut self, registers: DebugRegisters) -> Result<(), String> {
        self.debug_registers = registers;
        self.debug_dirty = true;
        Ok(())
    }

    pub fn extended_state(&self) -> Result<ExtendedState, String> {
        Ok(self.extended_state.clone())
    }

    pub fn set_extended_state(&mut self, state: &ExtendedState) -> Result<(), String> {
        self.extended_state = state.clone();
        self.extended_dirty = true;
        Ok(())
    }

    pub async fn run(&mut self) -> Result<Exit, String> {
        self.execute().await
    }

    pub async fn resume(&mut self) -> Result<Exit, String> {
        self.execute().await
    }

    async fn execute(&mut self) -> Result<Exit, String> {
        let sequence = NEXT_EXECUTION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let provenance = self.next_execution_provenance.take();
        let provider = provenance
            .as_ref()
            .map(|provenance| provenance.provider.as_str())
            .unwrap_or("<initial>");
        let caller_ret = provenance
            .as_ref()
            .map(|provenance| provenance.caller_ret)
            .unwrap_or(0);
        record_execution(sequence, ExecutionStage::Submit, self.pid, self.tid);
        trueos::logl::log(
            trueos::logl::level::IMPORTANT,
            format_args!(
                "WC3 EXEC SUBMIT seq={} pid={} tid={} provider={} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x}",
                sequence,
                self.pid,
                self.tid,
                provider,
                self.registers.eip,
                self.registers.esp,
                caller_ret,
            ),
        );
        let (reply, response) = oneshot::channel();
        self.commands
            .send(ThreadCommand::Execute {
                sequence,
                provenance,
                registers: self.registers_dirty.then_some(self.registers),
                debug_registers: self.debug_dirty.then_some(self.debug_registers),
                extended_state: self.extended_dirty.then(|| self.extended_state.clone()),
                reply,
            })
            .await
            .map_err(|_| "guest thread task closed".to_owned())?;
        self.registers_dirty = false;
        self.debug_dirty = false;
        self.extended_dirty = false;
        let event = match response.await {
            Ok(Ok(event)) => event,
            Ok(Err(error)) => {
                record_execution(sequence, ExecutionStage::Receive, self.pid, self.tid);
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "WC3 EXEC RECEIVE seq={} pid={} tid={} result=error error={:?}",
                        sequence, self.pid, self.tid, error,
                    ),
                );
                return Err(error);
            }
            Err(_) => {
                record_execution(sequence, ExecutionStage::Receive, self.pid, self.tid);
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "WC3 EXEC RECEIVE seq={} pid={} tid={} result=channel-closed",
                        sequence, self.pid, self.tid,
                    ),
                );
                return Err("guest thread task lost its exit".to_owned());
            }
        };
        record_execution(sequence, ExecutionStage::Receive, self.pid, self.tid);
        trueos::logl::log(
            trueos::logl::level::IMPORTANT,
            format_args!(
                "WC3 EXEC RECEIVE seq={} pid={} tid={} result=ok",
                sequence, self.pid, self.tid,
            ),
        );
        self.registers = event.exit.registers;
        self.debug_registers = event.debug_registers;
        self.extended_state = event.extended_state;
        Ok(event.exit)
    }
}

async fn guest_thread_task(
    mut context: Context,
    mut commands: mpsc::Receiver<ThreadCommand>,
    pid: u32,
    tid: u32,
) {
    let mut started = false;
    while let Some(command) = commands.recv().await {
        match command {
            ThreadCommand::Execute {
                sequence,
                provenance,
                registers,
                debug_registers,
                extended_state,
                reply,
            } => {
                let result = async {
                    let provider = provenance
                        .as_ref()
                        .map(|provenance| provenance.provider.as_str())
                        .unwrap_or("<initial>");
                    record_execution(sequence, ExecutionStage::Accept, pid, tid);
                    trueos::logl::log(
                        trueos::logl::level::IMPORTANT,
                        format_args!(
                            "WC3 EXEC ACCEPT seq={} pid={} tid={} provider={} stage=apply-state",
                            sequence, pid, tid, provider,
                        ),
                    );
                    if let Some(registers) = registers {
                        context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                    }
                    if let Some(registers) = debug_registers {
                        context
                            .set_debug_registers(registers)
                            .map_err(|error| error.to_string())?;
                    }
                    if let Some(state) = extended_state {
                        context
                            .set_extended_state(&state)
                            .map_err(|error| error.to_string())?;
                    }
                    record_execution(sequence, ExecutionStage::Enter, pid, tid);
                    trueos::logl::log(
                        trueos::logl::level::IMPORTANT,
                        format_args!(
                            "WC3 EXEC ENTER seq={} pid={} tid={} stage=x86-run",
                            sequence, pid, tid,
                        ),
                    );
                    let exit = match if started {
                        context.resume().await
                    } else {
                        started = true;
                        context.run().await
                    } {
                        Ok(exit) => exit,
                        Err(error) => {
                            record_execution(sequence, ExecutionStage::Exit, pid, tid);
                            trueos::logl::log(
                                trueos::logl::level::IMPORTANT,
                                format_args!(
                                    "WC3 EXEC EXIT seq={} pid={} tid={} kind=error detail={:?} eip=<unavailable> esp=<unavailable>",
                                    sequence, pid, tid, error,
                                ),
                            );
                            return Err(error.to_string());
                        }
                    };
                    record_execution(sequence, ExecutionStage::Exit, pid, tid);
                    trueos::logl::log(
                        trueos::logl::level::IMPORTANT,
                        format_args!(
                            "WC3 EXEC EXIT seq={} pid={} tid={} kind={:?} detail=0x{:08x} eip=0x{:08x} esp=0x{:08x}",
                            sequence,
                            pid,
                            tid,
                            exit.kind,
                            exit.detail,
                            exit.registers.eip,
                            exit.registers.esp,
                        ),
                    );
                    Ok(ThreadEvent {
                        exit,
                        debug_registers: context
                            .debug_registers()
                            .map_err(|error| error.to_string())?,
                        extended_state: context
                            .extended_state()
                            .map_err(|error| error.to_string())?,
                    })
                }
                .await;
                let state_capture = if result.is_ok() { "ok" } else { "failed" };
                let delivered = reply.send(result).is_ok();
                record_execution(sequence, ExecutionStage::Reply, pid, tid);
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "WC3 EXEC REPLY seq={} pid={} tid={} state_capture={} delivered={}",
                        sequence,
                        pid,
                        tid,
                        state_capture,
                        u8::from(delivered),
                    ),
                );
            }
        }
    }
}
