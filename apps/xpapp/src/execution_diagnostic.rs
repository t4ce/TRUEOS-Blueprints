//! Small coherent execution snapshots without the host pthread ABI.
//!
//! The gate protects only three atomic words. No callback, allocation, await,
//! or host call runs while it is held; guest scheduling never uses this gate.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ExecutionStage {
    Idle,
    Submit,
    Accept,
    Enter,
    Exit,
    Reply,
    Receive,
}

impl ExecutionStage {
    fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::Submit,
            2 => Self::Accept,
            3 => Self::Enter,
            4 => Self::Exit,
            5 => Self::Reply,
            6 => Self::Receive,
            _ => unreachable!("execution stage is written only from ExecutionStage"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionDiagnostic {
    pub sequence: u64,
    pub stage: ExecutionStage,
    pub pid: u32,
    pub tid: u32,
}

pub struct ExecutionSnapshot {
    gate: AtomicBool,
    sequence: AtomicU64,
    stage: AtomicU8,
    owner: AtomicU64,
}

struct SnapshotGuard<'a>(&'a AtomicBool);

impl Drop for SnapshotGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl ExecutionSnapshot {
    pub const fn new() -> Self {
        Self {
            gate: AtomicBool::new(false),
            sequence: AtomicU64::new(0),
            stage: AtomicU8::new(ExecutionStage::Idle as u8),
            owner: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> SnapshotGuard<'_> {
        while self.gate.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            while self.gate.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        SnapshotGuard(&self.gate)
    }

    pub fn store(&self, value: ExecutionDiagnostic) {
        let _guard = self.lock();
        self.sequence.store(value.sequence, Ordering::Relaxed);
        self.stage.store(value.stage as u8, Ordering::Relaxed);
        self.owner.store((u64::from(value.pid) << 32) | u64::from(value.tid), Ordering::Relaxed);
    }

    pub fn load(&self) -> ExecutionDiagnostic {
        let _guard = self.lock();
        let owner = self.owner.load(Ordering::Relaxed);
        ExecutionDiagnostic {
            sequence: self.sequence.load(Ordering::Relaxed),
            stage: ExecutionStage::from_byte(self.stage.load(Ordering::Relaxed)),
            pid: (owner >> 32) as u32,
            tid: owner as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Barrier, atomic::AtomicUsize};

    #[test]
    fn preserves_idle_and_every_stage_with_full_width_identifiers() {
        let snapshot = ExecutionSnapshot::new();
        assert_eq!(snapshot.load(), ExecutionDiagnostic {
            sequence: 0, stage: ExecutionStage::Idle, pid: 0, tid: 0,
        });
        for stage in [ExecutionStage::Idle, ExecutionStage::Submit, ExecutionStage::Accept,
            ExecutionStage::Enter, ExecutionStage::Exit, ExecutionStage::Reply, ExecutionStage::Receive]
        {
            let value = ExecutionDiagnostic { sequence: u64::MAX, stage, pid: u32::MAX, tid: 0x8000_0001 };
            snapshot.store(value);
            assert_eq!(snapshot.load(), value);
        }
    }

    #[test]
    fn concurrent_reads_never_mix_different_execution_snapshots() {
        let snapshot = ExecutionSnapshot::new();
        let start = Barrier::new(4);
        let writers = AtomicUsize::new(2);
        std::thread::scope(|threads| {
            for pid in [1u32, 2] {
                let (snapshot, start, writers) = (&snapshot, &start, &writers);
                threads.spawn(move || {
                    start.wait();
                    for tid in 1..=25_000 {
                        snapshot.store(ExecutionDiagnostic {
                            sequence: (u64::from(pid) << 32) | u64::from(tid),
                            stage: ExecutionStage::from_byte((tid % 7) as u8),
                            pid,
                            tid,
                        });
                        if tid % 32 == 0 { std::thread::yield_now(); }
                    }
                    writers.fetch_sub(1, Ordering::Release);
                });
            }
            let reader = threads.spawn(|| {
                start.wait();
                let mut reads = 0;
                loop {
                    let value = snapshot.load();
                    assert_eq!(value.sequence, (u64::from(value.pid) << 32) | u64::from(value.tid));
                    assert_eq!(value.stage, ExecutionStage::from_byte((value.tid % 7) as u8));
                    reads += 1;
                    if writers.load(Ordering::Acquire) == 0 { break; }
                }
                reads
            });
            start.wait();
            assert!(reader.join().unwrap() > 0);
        });
    }
}
