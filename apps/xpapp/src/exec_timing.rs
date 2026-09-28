//! Temporary aggregate execution markers. Deliberately independent of nolog.
//! No per-exit output; wall times include time descheduled. Provider time is
//! also contained in context_gap, so these records must not be added together.
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::BTreeMap;
use std::time::Duration;
use trueos::x86::{CarrierTiming, Exit};
use xpapp::child_loader::ProviderOp;

// Coordinator execution is serial. These cumulative counters are sampled only
// at successful frame publication, so no clock read is added to the carrier
// or provider hot path beyond the measurements already made there.
static FRAME_PREPARE_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_REQUEST_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_NATIVE_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_REPLY_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_PROVIDER_NON_DRAW_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_BATCH_REPLAY_NS: AtomicU64 = AtomicU64::new(0);
static FRAME_CARRIER_EXITS: AtomicU64 = AtomicU64::new(0);
static FRAME_NON_DRAW_CALLS: AtomicU64 = AtomicU64::new(0);

fn ns(duration: Duration) -> u64 { duration.as_nanos().min(u64::MAX as u128) as u64 }

#[derive(Clone, Copy, Default)]
struct FrameTotals {
    prepare: u64, request: u64, native: u64, reply: u64,
    provider_non_draw: u64, batch_replay: u64,
    carrier_exits: u64, non_draw_calls: u64,
}
impl FrameTotals {
    fn snapshot() -> Self {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        Self {
            prepare: load(&FRAME_PREPARE_NS), request: load(&FRAME_REQUEST_NS),
            native: load(&FRAME_NATIVE_NS), reply: load(&FRAME_REPLY_NS),
            provider_non_draw: load(&FRAME_PROVIDER_NON_DRAW_NS),
            batch_replay: load(&FRAME_BATCH_REPLAY_NS),
            carrier_exits: load(&FRAME_CARRIER_EXITS),
            non_draw_calls: load(&FRAME_NON_DRAW_CALLS),
        }
    }
    fn since(self, old: Self) -> Self {
        Self {
            prepare: self.prepare.saturating_sub(old.prepare),
            request: self.request.saturating_sub(old.request),
            native: self.native.saturating_sub(old.native),
            reply: self.reply.saturating_sub(old.reply),
            provider_non_draw: self.provider_non_draw.saturating_sub(old.provider_non_draw),
            batch_replay: self.batch_replay.saturating_sub(old.batch_replay),
            carrier_exits: self.carrier_exits.saturating_sub(old.carrier_exits),
            non_draw_calls: self.non_draw_calls.saturating_sub(old.non_draw_calls),
        }
    }
}

#[derive(Default)]
pub struct FrameAttribution {
    previous: BTreeMap<(u32, u32), (std::time::Instant, FrameTotals)>,
    last_report: BTreeMap<(u32, u32), std::time::Instant>,
}
impl FrameAttribution {
    pub fn published(&mut self, pid: u32, hwnd: u32, at: std::time::Instant,
        draw: Duration, swap: Duration) {
        let totals = FrameTotals::snapshot();
        let key = (pid, hwnd);
        let previous = self.previous.insert(key, (at, totals));
        let Some((old_at, old)) = previous else {
            self.last_report.insert(key, at);
            return;
        };
        if self.last_report.get(&key).is_some_and(|last| at.saturating_duration_since(*last) < Duration::from_secs(1)) {
            return;
        }
        self.last_report.insert(key, at);
        let frame = at.saturating_duration_since(old_at);
        let outside = frame.saturating_sub(draw).saturating_sub(swap);
        let delta = totals.since(old);
        let known = delta.prepare.saturating_add(delta.request)
            .saturating_add(delta.native).saturating_add(delta.reply)
            .saturating_add(delta.provider_non_draw).saturating_add(delta.batch_replay);
        let outside_ns = ns(outside);
        crate::logl::emit(trueos::logl::level::IMPORTANT, format_args!(
            "XPAPP FRAME OUTSIDE pid={} hwnd=0x{:08x} frame_ms={:.3} outside_draw_swap_ms={:.3} prepare_ms={:.3} carrier_request_ms={:.3} guest_kernel_native_ms={:.3} carrier_reply_ms={:.3} provider_non_draw_ms={:.3} gl_state_replay_ms={:.3} residual_ms={:.3} accounting_excess_ms={:.3} carrier_exits={} provider_non_draw_calls={} scope=single-active-drawable-serial-child-wall-all-threads-provider-excludes-draw-and-swap-residual-includes-scheduling-and-coordinator",
            pid, hwnd, ns(frame) as f64 / 1e6, outside_ns as f64 / 1e6,
            delta.prepare as f64 / 1e6, delta.request as f64 / 1e6,
            delta.native as f64 / 1e6, delta.reply as f64 / 1e6,
            delta.provider_non_draw as f64 / 1e6, delta.batch_replay as f64 / 1e6,
            outside_ns.saturating_sub(known) as f64 / 1e6,
            known.saturating_sub(outside_ns) as f64 / 1e6,
            delta.carrier_exits, delta.non_draw_calls,
        ));
    }
}

pub fn record_light_batch_replay(duration: Duration) {
    FRAME_BATCH_REPLAY_NS.fetch_add(ns(duration), Ordering::Relaxed);
}

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
        if pid != xpapp::session::LAUNCHER_PID {
            FRAME_PREPARE_NS.fetch_add(values[1], Ordering::Relaxed);
            FRAME_REQUEST_NS.fetch_add(values[2], Ordering::Relaxed);
            FRAME_NATIVE_NS.fetch_add(values[3], Ordering::Relaxed);
            FRAME_REPLY_NS.fetch_add(values[4], Ordering::Relaxed);
            FRAME_CARRIER_EXITS.fetch_add(1, Ordering::Relaxed);
        }
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
    by_provider: BTreeMap<(u32, u32), ProviderTotal>,
    max_ns: u64, pid: u32, tid: u32, id: u32, eip: u32, op: Option<ProviderOp>,
}
#[derive(Default)]
struct ProviderTotal { count: u64, ns: u64, max_ns: u64, op: Option<ProviderOp> }
static PROVIDERS: Mutex<Providers> = Mutex::new(Providers {
    since: None, count: 0, total_ns: 0, by_provider: BTreeMap::new(), max_ns: 0, pid: 0, tid: 0, id: 0, eip: 0, op: None,
});
pub struct ProviderScope { start: std::time::Instant, pid: u32, tid: u32, id: u32, eip: u32, op: Option<ProviderOp> }
impl ProviderScope {
    pub fn new(pid: u32, tid: u32, id: u32, eip: u32, op: Option<ProviderOp>) -> Self {
        Self { start: std::time::Instant::now(), pid, tid, id, eip, op }
    }
}
impl Drop for ProviderScope {
    fn drop(&mut self) {
        let now = std::time::Instant::now();
        let ns = now.duration_since(self.start).as_nanos() as u64;
        if self.pid != xpapp::session::LAUNCHER_PID
            && !matches!(self.op, Some(ProviderOp::GlDrawElements | ProviderOp::WglSwapLayerBuffers))
        {
            FRAME_PROVIDER_NON_DRAW_NS.fetch_add(ns, Ordering::Relaxed);
            FRAME_NON_DRAW_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        let report = {
            let mut totals = PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
            let since = *totals.since.get_or_insert(self.start);
            totals.count += 1;
            totals.total_ns = totals.total_ns.saturating_add(ns);
            let entry = totals.by_provider.entry((self.pid, self.id)).or_default();
            entry.count += 1;
            entry.ns = entry.ns.saturating_add(ns);
            entry.max_ns = entry.max_ns.max(ns);
            entry.op = self.op;
            if ns >= totals.max_ns {
                totals.max_ns = ns;
                totals.pid = self.pid; totals.tid = self.tid;
                totals.id = self.id; totals.eip = self.eip; totals.op = self.op;
            }
            if now.duration_since(since) >= Duration::from_secs(2) {
                Some(std::mem::take(&mut *totals))
            } else { None }
        };
        if let Some(t) = report {
            crate::logl::emit(trueos::logl::level::IMPORTANT, format_args!(
                "XPAPP PROVIDER TIME samples={} total_us={} max_us={} max_pid={} max_tid={} max_id={} max_eip=0x{:08x} max_op={:?} scope=vmcall-dispatch-wall-including-awaits-and-special-traps",
                t.count, t.total_ns/1000, t.max_ns/1000, t.pid, t.tid, t.id, t.eip, t.op));
            let mut ranked: Vec<_> = t.by_provider.into_iter().collect();
            ranked.sort_unstable_by(|a, b| b.1.ns.cmp(&a.1.ns));
            for ((pid, id), p) in ranked.into_iter().take(8) {
                crate::logl::emit(trueos::logl::level::IMPORTANT, format_args!(
                    "XPAPP PROVIDER TOP pid={} id={} op={:?} calls={} total_us={} max_us={} scope=subset-of-provider-total-overlaps-frame-draw",
                    pid, id, p.op, p.count, p.ns / 1000, p.max_ns / 1000));
            }
        }
    }
}
