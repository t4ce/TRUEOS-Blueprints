//! Direct coordinator-owned x86 contexts.  A context is resumed only through
//! `&mut self`; `Context::run`/`resume` already await the native carrier, so
//! no actor, channel, or response handoff is needed between ordinary exits.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use trueos::x86::{Context, DebugRegisters, Exit, ExtendedState, Registers};

macro_rules! trace_api {
    ($message:expr $(,)?) => {
        if cfg!(feature = "trace-api") {
            crate::logl::log!(trueos::logl::level::IMPORTANT, $message);
        }
    };
}

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

/// Coordinator-side stopped context.  Debug and extended state are read only
/// by consumers which need it.  Pending writes retain the previous actor's
/// "apply at the next execution permit" behavior without an actor handoff.
pub struct GuestThreadContext {
    context: Context,
    pid: u32,
    tid: u32,
    registers: Registers,
    registers_dirty: bool,
    pending_debug_registers: Option<DebugRegisters>,
    pending_extended_state: Option<ExtendedState>,
    next_execution_provenance: Option<ExecutionProvenance>,
    started: bool,
}

impl GuestThreadContext {
    pub fn spawn(context: Context, pid: u32, tid: u32) -> Result<Self, String> {
        let registers = context.registers().map_err(|error| error.to_string())?;
        Ok(Self {
            context,
            pid,
            tid,
            registers,
            registers_dirty: false,
            pending_debug_registers: None,
            pending_extended_state: None,
            next_execution_provenance: None,
            started: false,
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
        match self.pending_debug_registers {
            Some(registers) => Ok(registers),
            None => self
                .context
                .debug_registers()
                .map_err(|error| error.to_string()),
        }
    }

    pub fn set_debug_registers(&mut self, registers: DebugRegisters) -> Result<(), String> {
        self.pending_debug_registers = Some(registers);
        Ok(())
    }

    pub fn extended_state(&self) -> Result<ExtendedState, String> {
        match &self.pending_extended_state {
            Some(state) => Ok(state.clone()),
            None => self
                .context
                .extended_state()
                .map_err(|error| error.to_string()),
        }
    }

    pub fn set_extended_state(&mut self, state: &ExtendedState) -> Result<(), String> {
        self.pending_extended_state = Some(state.clone());
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
        trace_api!(format_args!(
            "WC3 EXEC SUBMIT seq={} pid={} tid={} provider={} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x}",
            sequence,
            self.pid,
            self.tid,
            provider,
            self.registers.eip,
            self.registers.esp,
            caller_ret,
        ),);
        record_execution(sequence, ExecutionStage::Accept, self.pid, self.tid);
        if self.registers_dirty {
            self.context
                .set_registers(self.registers)
                .map_err(|error| error.to_string())?;
        }
        if let Some(registers) = self.pending_debug_registers {
            self.context
                .set_debug_registers(registers)
                .map_err(|error| error.to_string())?;
        }
        if let Some(state) = self.pending_extended_state.as_ref() {
            self.context
                .set_extended_state(state)
                .map_err(|error| error.to_string())?;
        }
        record_execution(sequence, ExecutionStage::Enter, self.pid, self.tid);
        let exit = match if self.started {
            self.context.resume().await
        } else {
            self.started = true;
            self.context.run().await
        } {
            Ok(exit) => exit,
            Err(error) => {
                record_execution(sequence, ExecutionStage::Exit, self.pid, self.tid);
                crate::logl::log!(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "WC3 EXEC EXIT seq={} pid={} tid={} kind=error detail={:?} eip=<unavailable> esp=<unavailable>",
                        sequence, self.pid, self.tid, error,
                    ),
                );
                return Err(error.to_string());
            }
        };
        record_execution(sequence, ExecutionStage::Exit, self.pid, self.tid);
        trace_api!(format_args!(
            "WC3 EXEC EXIT seq={} pid={} tid={} kind={:?} detail=0x{:08x} eip=0x{:08x} esp=0x{:08x}",
            sequence,
            self.pid,
            self.tid,
            exit.kind,
            exit.detail,
            exit.registers.eip,
            exit.registers.esp,
        ),);
        self.registers = exit.registers;
        self.registers_dirty = false;
        self.pending_debug_registers = None;
        self.pending_extended_state = None;
        record_execution(sequence, ExecutionStage::Reply, self.pid, self.tid);
        record_execution(sequence, ExecutionStage::Receive, self.pid, self.tid);
        trace_api!(format_args!(
            "WC3 EXEC RECEIVE seq={} pid={} tid={} result=ok",
            sequence, self.pid, self.tid,
        ),);
        Ok(exit)
    }
}
