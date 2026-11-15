//! Optional per-job wall timings. This module is absent from normal builds.
use std::cmp::Reverse;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use trueos_kokoro_aot::{OpCode, Program};
use trueos_kokoro_dispatch::{Attributes, DispatchError, ExecutionProfile, OpTiming, decode};

static EPOCH: OnceLock<Instant> = OnceLock::new();
static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

pub(crate) fn clock_us() -> u64 {
    EPOCH
        .get_or_init(Instant::now)
        .elapsed()
        .as_micros()
        .min(u64::MAX as u128) as u64
}

fn emit(message: std::fmt::Arguments<'_>) {
    #[cfg(target_os = "trueos")]
    crate::diagnostics::detail(false, message);
    #[cfg(not(target_os = "trueos"))]
    eprintln!("{message}");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CoreLocation {
    kind: &'static str,
    core_type: u8,
    apic: Option<u32>,
}

impl CoreLocation {
    fn detect() -> Self {
        let mut location = Self {
            kind: "unknown",
            core_type: 0,
            apic: None,
        };
        #[cfg(target_arch = "x86_64")]
        {
            use core::arch::x86_64::{__cpuid, __cpuid_count};
            // CPUID is supported on x86_64; guard every optional leaf. TRUEOS
            // forwards these topology/core-type leaves from the executing AP.
            {
                let maximum = __cpuid(0).eax;
                if maximum >= 1 {
                    location.apic = Some(__cpuid(1).ebx >> 24);
                }
                for leaf in [0x1f, 0x0b] {
                    if maximum >= leaf {
                        let topology = __cpuid_count(leaf, 0);
                        if topology.ebx != 0 {
                            location.apic = Some(topology.edx);
                            break;
                        }
                    }
                }
                if maximum >= 0x1a {
                    location.core_type = (__cpuid(0x1a).eax >> 24) as u8;
                    location.kind = match location.core_type {
                        0x40 => "perf",
                        0x20 => "eff",
                        _ => "unknown",
                    };
                }
            }
        }
        location
    }
}

struct FamilyTiming {
    opcode: OpCode,
    timing: OpTiming,
}

/// Owns one dispatch accumulator for all chunks of a speech request. Report
/// buffers are reserved up front so finishing does not allocate sorting tables.
pub(crate) struct JobProfile {
    run: u64,
    program: Program<'static>,
    execution: ExecutionProfile,
    stage_calls: [u64; 5],
    stage_us: [u64; 5],
    families: Vec<FamilyTiming>,
    ranked_ops: Vec<usize>,
    started: u64,
    start_core: CoreLocation,
    reported: bool,
}

impl JobProfile {
    pub(crate) fn new(program: Program<'static>, text_bytes: usize) -> Result<Self, DispatchError> {
        let count = program.op_count() as usize;
        let execution = ExecutionProfile::new(count, clock_us)?;
        let mut families = Vec::new();
        families
            .try_reserve_exact(count)
            .map_err(|_| DispatchError::Allocation)?;
        let mut ranked_ops = Vec::new();
        ranked_ops
            .try_reserve_exact(count)
            .map_err(|_| DispatchError::Allocation)?;
        ranked_ops.extend(0..count);
        let run = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
        let start_core = CoreLocation::detect();
        emit(format_args!(
            "ttstt: profile run={run} event=start text_bytes={text_bytes} core={} core_type=0x{:02x} apic={:?}",
            start_core.kind, start_core.core_type, start_core.apic,
        ));
        Ok(Self {
            run,
            program,
            execution,
            stage_calls: [0; 5],
            stage_us: [0; 5],
            families,
            ranked_ops,
            started: clock_us(),
            start_core,
            reported: false,
        })
    }

    pub(crate) fn execution(&mut self) -> &mut ExecutionProfile {
        &mut self.execution
    }

    pub(crate) fn record_stage(&mut self, stage: usize, started: u64) {
        self.stage_calls[stage] += 1;
        self.stage_us[stage] += clock_us().saturating_sub(started);
    }

    pub(crate) fn finish(&mut self, status: &str) {
        if self.reported {
            return;
        }
        self.reported = true;
        let elapsed = clock_us().saturating_sub(self.started);
        let end_core = CoreLocation::detect();
        let entries = self.execution.entries();
        let mut total = 0u64;
        let mut kernel = 0u64;
        for (index, timing) in entries.iter().enumerate().filter(|(_, t)| t.calls != 0) {
            let Some(op) = self.program.op(index as u32) else {
                continue;
            };
            total += timing.total_us;
            kernel += timing.kernel_us;
            if let Some(family) = self.families.iter_mut().find(|f| f.opcode == op.opcode) {
                family.timing.calls += timing.calls;
                family.timing.total_us += timing.total_us;
                family.timing.kernel_us += timing.kernel_us;
            } else {
                self.families.push(FamilyTiming {
                    opcode: op.opcode,
                    timing: *timing,
                });
            }
        }
        emit(format_args!(
            "ttstt: profile run={} event=end status={status} wall_us={elapsed} core={} core_type=0x{:02x} apic={:?} core_changed={}",
            self.run,
            end_core.kind,
            end_core.core_type,
            end_core.apic,
            (end_core != self.start_core) as u8,
        ));
        emit(format_args!(
            "ttstt: profile run={} stages_us frontend={} prepare_chunk={} execute={} pcm={} finish={} stage_calls={:?} pcm_includes_sink=1 dispatch_us={total} kernel_us={kernel} dispatch_overhead_us={}",
            self.run,
            self.stage_us[0],
            self.stage_us[1],
            self.stage_us[2],
            self.stage_us[3],
            self.stage_us[4],
            self.stage_calls,
            total.saturating_sub(kernel),
        ));
        self.families
            .sort_unstable_by_key(|family| Reverse(family.timing.total_us));
        for family in &self.families {
            emit(format_args!(
                "ttstt: profile run={} opcode={:?} calls={} total_us={} kernel_us={} dispatch_percent={:.2}",
                self.run,
                family.opcode,
                family.timing.calls,
                family.timing.total_us,
                family.timing.kernel_us,
                if total == 0 {
                    0.0
                } else {
                    family.timing.total_us as f64 * 100.0 / total as f64
                },
            ));
        }
        self.ranked_ops
            .sort_unstable_by_key(|&index| Reverse(entries[index].total_us));
        for &index in self
            .ranked_ops
            .iter()
            .filter(|&&i| entries[i].calls != 0)
            .take(10)
        {
            let Some(op) = self.program.op(index as u32) else {
                continue;
            };
            let timing = entries[index];
            let profile = self
                .program
                .op_attributes(op)
                .and_then(|record| decode(record, op.opcode).ok())
                .and_then(|attributes| match attributes {
                    Attributes::MatMul(a) => Some(u32::from(a.profile)),
                    Attributes::DynamicQuantizedGemm(a) => Some(u32::from(a.profile)),
                    Attributes::DynamicQuantizedConv1d(a) => Some(u32::from(a.profile)),
                    Attributes::BiLstm256(a) => Some(u32::from(a.profile)),
                    Attributes::FloatConv(a) => Some(u32::from(a.profile)),
                    Attributes::Resize(a) => Some(u32::from(a.profile)),
                    _ => None,
                });
            emit(format_args!(
                "ttstt: profile run={} top_op={} opcode={:?} phase={:?} attribute_profile={:?} calls={} total_us={} kernel_us={}",
                self.run,
                index,
                op.opcode,
                op.phase,
                profile,
                timing.calls,
                timing.total_us,
                timing.kernel_us,
            ));
        }
    }
}

impl Drop for JobProfile {
    fn drop(&mut self) {
        // The outer service can time out between slices without receiving a
        // terminal JobProgress. Keep that incomplete job's accumulated evidence.
        self.finish("dropped-before-completion");
    }
}
