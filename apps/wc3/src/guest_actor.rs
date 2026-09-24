//! A Windows thread's x86 context is owned by one Tokio task. The coordinator
//! keeps only stopped-state snapshots and grants execution one exit at a time.

use tokio::sync::{mpsc, oneshot};
use trueos::x86::{Context, DebugRegisters, Exit, ExtendedState, Registers};

enum ThreadCommand {
    Execute {
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
    registers: Registers,
    debug_registers: DebugRegisters,
    extended_state: ExtendedState,
    registers_dirty: bool,
    debug_dirty: bool,
    extended_dirty: bool,
}

impl GuestThreadContext {
    pub fn spawn(context: Context) -> Result<Self, String> {
        let registers = context.registers().map_err(|error| error.to_string())?;
        let debug_registers = context
            .debug_registers()
            .map_err(|error| error.to_string())?;
        let extended_state = context
            .extended_state()
            .map_err(|error| error.to_string())?;
        let (commands, receiver) = mpsc::channel(1);
        let task = tokio::spawn(guest_thread_task(context, receiver));
        Ok(Self {
            commands,
            _task: task,
            registers,
            debug_registers,
            extended_state,
            registers_dirty: false,
            debug_dirty: false,
            extended_dirty: false,
        })
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
        let (reply, response) = oneshot::channel();
        self.commands
            .send(ThreadCommand::Execute {
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
        let event = response
            .await
            .map_err(|_| "guest thread task lost its exit".to_owned())??;
        self.registers = event.exit.registers;
        self.debug_registers = event.debug_registers;
        self.extended_state = event.extended_state;
        Ok(event.exit)
    }
}

async fn guest_thread_task(mut context: Context, mut commands: mpsc::Receiver<ThreadCommand>) {
    let mut started = false;
    while let Some(command) = commands.recv().await {
        match command {
            ThreadCommand::Execute {
                registers,
                debug_registers,
                extended_state,
                reply,
            } => {
                let result = async {
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
                    let exit = if started {
                        context.resume().await
                    } else {
                        started = true;
                        context.run().await
                    }
                    .map_err(|error| error.to_string())?;
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
                let _ = reply.send(result);
            }
        }
    }
}
