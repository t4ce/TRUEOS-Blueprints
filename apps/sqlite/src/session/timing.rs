//! Bounded numeric service timing. Never record SQL, filenames or values.
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering}};
use super::Instant;

static NEXT: AtomicU64 = AtomicU64::new(1);
static TIMING_ALL: OnceLock<bool> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
static NEXT_SAMPLE_MS: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);

fn admit_sample(now_ms: u64, next: &AtomicU64) -> bool {
    let mut deadline = next.load(Ordering::Relaxed);
    loop {
        if now_ms < deadline { return false; }
        match next.compare_exchange_weak(deadline, now_ms.saturating_add(250), Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(updated) => deadline = updated,
        }
    }
}

pub(super) fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(super) struct RequestTiming {
    seq: u64,
    operation: &'static str,
    connection_id: u64,
    request_id: u64,
    started: Instant,
    pub phase: &'static str,
    pub lock_us: u64,
    pub queue_us: u64,
    pub before_us: u64,
    pub before_bytes: u64,
    pub execute_us: u64,
    pub reclaim_us: u64,
    pub after_us: u64,
    pub after_bytes: u64,
    pub publish_us: u64,
    pub publish_bytes: u64,
    pub restore_us: u64,
    pub reply_serialize_us: u64,
    pub reply_write_us: u64,
    pub reply_bytes: u64,
    pub completed: bool,
}
impl RequestTiming {
    pub(super) fn new(operation: &'static str, connection_id: Option<u64>, request_id: Option<u64>) -> Self {
        Self { seq: NEXT.fetch_add(1, Ordering::Relaxed), operation,
            connection_id: connection_id.unwrap_or(0), request_id: request_id.unwrap_or(0),
            started: Instant::now(), phase: "dispatch", lock_us: 0, queue_us: 0, before_us: 0, before_bytes: 0,
            execute_us: 0, reclaim_us: 0, after_us: 0, after_bytes: 0, publish_us: 0,
            publish_bytes: 0, restore_us: 0, reply_serialize_us: 0, reply_write_us: 0,
            reply_bytes: 0, completed: false }
    }
}
impl Drop for RequestTiming {
    fn drop(&mut self) {
        let elapsed_us = elapsed_us(self.started);
        let all = *TIMING_ALL.get_or_init(|| std::env::var("TRUEOS_SQLITE_TIMING_ALL").as_deref() == Ok("1"));
        if !all && self.seq > 16 {
            if self.seq % 64 != 0 && elapsed_us < 100_000 && self.completed { return; }
            let now_ms = EPOCH.get_or_init(Instant::now).elapsed().as_millis().min(u64::MAX as u128) as u64;
            if !admit_sample(now_ms, &NEXT_SAMPLE_MS) {
                SKIPPED.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        let skipped = SKIPPED.swap(0, Ordering::Relaxed);
        let line = format!("sqlite-service-timing seq={} connection_id={} request_id={} operation={} phase={} elapsed_us={} lock_us={} queue_us={} before_us={} before_bytes={} execute_us={} reclaim_us={} after_us={} after_bytes={} publish_us={} publish_bytes={} restore_us={} reply_serialize_us={} reply_write_us={} reply_bytes={} completed={} skipped={}",
            self.seq, self.connection_id, self.request_id, self.operation, self.phase, elapsed_us, self.lock_us,
            self.queue_us, self.before_us, self.before_bytes, self.execute_us, self.reclaim_us,
            self.after_us, self.after_bytes, self.publish_us, self.publish_bytes, self.restore_us,
            self.reply_serialize_us, self.reply_write_us, self.reply_bytes, self.completed, skipped);
        #[cfg(target_os = "trueos")]
        trueos::logl::log(trueos::logl::level::INFO, line.as_str());
        #[cfg(not(target_os = "trueos"))]
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_rate_gate_admits_at_most_one_sample_per_250_ms() {
        let next = AtomicU64::new(0);
        assert!(admit_sample(0, &next));
        for now in 0..250 { assert!(!admit_sample(now, &next)); }
        assert!(admit_sample(250, &next));
        assert!(!admit_sample(250, &next));
        assert!(!admit_sample(499, &next));
        assert!(admit_sample(500, &next));
    }
}
