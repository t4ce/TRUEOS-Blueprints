//! Optional caller-clocked wall timings; disabled in the normal build.
use alloc::vec::Vec;
use crate::DispatchError;

#[derive(Clone, Copy, Default)]
pub struct OpTiming {
    pub calls: u64,
    pub total_us: u64,
    pub kernel_us: u64,
}

pub struct ExecutionProfile {
    clock: fn() -> u64,
    ops: Vec<OpTiming>,
}

impl ExecutionProfile {
    pub fn new(op_count: usize, clock: fn() -> u64) -> Result<Self, DispatchError> {
        let mut ops = Vec::new();
        ops.try_reserve_exact(op_count).map_err(|_| DispatchError::Allocation)?;
        ops.resize(op_count, OpTiming::default());
        Ok(Self { clock, ops })
    }

    pub fn entries(&self) -> &[OpTiming] { &self.ops }

    pub(crate) fn clock(&self) -> fn() -> u64 { self.clock }

    pub(crate) fn record_total(&mut self, op: u32, elapsed: u64) {
        if let Some(entry) = self.ops.get_mut(op as usize) {
            entry.calls += 1;
            entry.total_us += elapsed;
        }
    }

    pub(crate) fn record_kernel(&mut self, op: u32, elapsed: u64) {
        if let Some(entry) = self.ops.get_mut(op as usize) { entry.kernel_us += elapsed; }
    }
}
