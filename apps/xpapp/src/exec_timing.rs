//! Temporary aggregate execution markers. Deliberately independent of nolog.
//! No per-exit output; wall times include time descheduled. Provider time is
//! also contained in context_gap, so these records must not be added together.
use std::sync::Mutex;
use std::time::Duration;
use trueos::x86::{CarrierTiming, Exit};

#[derive(Default)]
pub struct Window {
    since: Option<std::time::Instant>,
    previous: Option<std::time::Instant>,
    count: u64,
    sums: [u64; 5],
    max_roundtrip_ns: u64,
}
impl Window {
    pub fn record(&mut self, pid: u32, tid: u32, prepare: std::time::Instant, prepared: std::time::Instant,
        timing: CarrierTiming, exit: &Exit) {
        let now = std::time::Instant::now();
        let since = *self.since.get_or_insert(prepare);
        let gap = self.previous.map_or(0, |last| prepare.saturating_duration_since(last).as_nanos() as u64);
        let values = [gap, prepared.duration_since(prepare).as_nanos() as u64,
            timing.request_ns, timing.native_ns, timing.reply_ns];
        for (sum, value) in self.sums.iter_mut().zip(values) { *sum = sum.saturating_add(value); }
        self.count += 1;
        self.max_roundtrip_ns = self.max_roundtrip_ns.max(timing.request_ns.saturating_add(timing.native_ns).saturating_add(timing.reply_ns));
        if now.duration_since(since) >= Duration::from_secs(2) {
            crate::logl::emit(trueos::logl::level::IMPORTANT, format_args!(
                "XPAPP EXEC TIME pid={} tid={} samples={} window_ms={} context_gap_us={} prepare_us={} request_us={} native_us={} reply_us={} max_roundtrip_us={} eip=0x{:08x} kind={:?} detail={} scope=wall-context-gap-includes-other-threads",
                pid, tid, self.count, now.duration_since(since).as_millis(),
                self.sums[0]/1000, self.sums[1]/1000, self.sums[2]/1000,
                self.sums[3]/1000, self.sums[4]/1000, self.max_roundtrip_ns/1000,
                exit.registers.eip, exit.kind, exit.detail));
            self.since = Some(std::time::Instant::now());
            self.count = 0;
            self.sums = [0; 5];
            self.max_roundtrip_ns = 0;
        }
        // Exclude this marker's log emission from the next context gap.
        self.previous = Some(std::time::Instant::now());
    }
}

#[derive(Default)]
struct Providers {
    since: Option<std::time::Instant>, count: u64, total_ns: u64,
    max_ns: u64, pid: u32, tid: u32, id: u32, eip: u32,
}
static PROVIDERS: Mutex<Providers> = Mutex::new(Providers {
    since: None, count: 0, total_ns: 0, max_ns: 0, pid: 0, tid: 0, id: 0, eip: 0,
});
pub struct ProviderScope { start: std::time::Instant, pid: u32, tid: u32, id: u32, eip: u32 }
impl ProviderScope {
    pub fn new(pid: u32, tid: u32, id: u32, eip: u32) -> Self {
        Self { start: std::time::Instant::now(), pid, tid, id, eip }
    }
}
impl Drop for ProviderScope {
    fn drop(&mut self) {
        let now = std::time::Instant::now();
        let ns = now.duration_since(self.start).as_nanos() as u64;
        let report = {
            let mut totals = PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
            let since = *totals.since.get_or_insert(self.start);
            totals.count += 1;
            totals.total_ns = totals.total_ns.saturating_add(ns);
            if ns >= totals.max_ns {
                totals.max_ns = ns;
                totals.pid = self.pid; totals.tid = self.tid;
                totals.id = self.id; totals.eip = self.eip;
            }
            if now.duration_since(since) >= Duration::from_secs(2) {
                Some(std::mem::take(&mut *totals))
            } else { None }
        };
        if let Some(t) = report {
            crate::logl::emit(trueos::logl::level::IMPORTANT, format_args!(
                "XPAPP PROVIDER TIME samples={} total_us={} max_us={} max_pid={} max_tid={} max_id={} max_eip=0x{:08x} scope=vmcall-dispatch-wall-including-awaits-and-special-traps",
                t.count, t.total_ns/1000, t.max_ns/1000, t.pid, t.tid, t.id, t.eip));
        }
    }
}
