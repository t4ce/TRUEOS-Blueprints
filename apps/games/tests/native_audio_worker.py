#!/usr/bin/env python3
"""Exercise Games' real native sound-worker lifecycle without audio hardware."""
from pathlib import Path
import subprocess
import tempfile


def main():
    source = (Path(__file__).resolve().parents[1] / "src/audio.rs").read_text()
    lifecycle = source[source.index("pub struct Audio {"):source.index("struct AudioPump {")]
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
const WORKER_TICK_MS: u64 = 5;
#[derive(Default)] struct World {
    reject: bool, cancelled: bool, cancel_after: usize,
    updates: Vec<u64>, sleeps: Vec<u64>, drops: usize, now: u64,
}
static WORLD: Mutex<World> = Mutex::new(World {
    reject: false, cancelled: false, cancel_after: 3,
    updates: Vec::new(), sleeps: Vec::new(), drops: 0, now: 0,
});
static JOBS: Mutex<Vec<Box<dyn FnOnce() + Send>>> = Mutex::new(Vec::new());
mod level { pub const WARN: u8 = 1; }
mod logl { pub fn log(_: u8, _: impl core::fmt::Display) {} }
mod clock { pub fn monotonic_millis() -> u64 { crate::WORLD.lock().unwrap().now } }
mod vsys {
    pub fn sleep_ms(ms: u64) {
        let mut w = crate::WORLD.lock().unwrap();
        w.sleeps.push(ms); w.now += ms;
        if w.sleeps.len() >= w.cancel_after { w.cancelled = true; }
    }
}
mod v { pub mod worker_abi {
    pub unsafe fn trueos_service_lane_submit_job(job: Box<dyn FnOnce() + Send>) -> i32 {
        if crate::WORLD.lock().unwrap().reject { drop(job); -2 }
        else { crate::JOBS.lock().unwrap().push(job); 0 }
    }
    pub unsafe fn trueos_service_lane_cancellation_requested() -> bool {
        crate::WORLD.lock().unwrap().cancelled
    }
} }
struct AudioPump;
impl AudioPump {
    fn new() -> Self { Self }
    fn update(&mut self, now: u64) { WORLD.lock().unwrap().updates.push(now); }
}
impl Drop for AudioPump { fn drop(&mut self) { WORLD.lock().unwrap().drops += 1; } }
@LIFECYCLE@
fn reset() {
    JOBS.lock().unwrap().clear();
    *WORLD.lock().unwrap() = World { cancel_after: 3, ..Default::default() };
}
fn run_queued() { let job = JOBS.lock().unwrap().pop().unwrap(); job(); }
#[test] fn rejected_worker_drops_captures_and_uses_inline_pump() {
    reset(); WORLD.lock().unwrap().reject = true;
    let mut audio = Audio::new();
    assert!(audio.worker.is_none()); assert!(audio.inline.is_some());
    assert_eq!(Arc::strong_count(&audio.stop), 1);
    assert!(JOBS.lock().unwrap().is_empty());
    audio.update(70); assert_eq!(WORLD.lock().unwrap().updates, vec![70]);
}
#[test] fn accepted_worker_runs_separately_and_completion_enables_inline_fallback() {
    reset(); let mut audio = Audio::new();
    let finished = audio.worker.as_ref().unwrap().clone();
    assert!(audio.inline.is_none()); assert!(!finished.load(Ordering::Acquire));
    audio.update(70); assert!(WORLD.lock().unwrap().updates.is_empty());
    run_queued();
    assert!(finished.load(Ordering::Acquire));
    { let w = WORLD.lock().unwrap();
      assert_eq!(w.updates, vec![0, 5, 10]); assert_eq!(w.sleeps, vec![5, 5, 5]);
      assert_eq!(w.drops, 1, "audio device/pump drops before completion is observed"); }
    audio.update(80); assert!(audio.worker.is_none()); assert!(audio.inline.is_some());
    assert_eq!(WORLD.lock().unwrap().updates, vec![0, 5, 10, 80]);
}
#[test] fn dropping_audio_cooperatively_stops_an_accepted_worker() {
    reset(); let audio = Audio::new();
    let stop = audio.stop.clone(); let finished = audio.worker.as_ref().unwrap().clone();
    drop(audio); assert!(stop.load(Ordering::Acquire));
    run_queued(); assert!(finished.load(Ordering::Acquire));
    let w = WORLD.lock().unwrap(); assert!(w.updates.is_empty()); assert!(w.sleeps.is_empty());
}
#[test] fn kernel_cancellation_ends_native_job_without_an_extra_audio_write() {
    reset(); let audio = Audio::new();
    let finished = audio.worker.as_ref().unwrap().clone();
    WORLD.lock().unwrap().cancelled = true;
    run_queued(); assert!(finished.load(Ordering::Acquire));
    let w = WORLD.lock().unwrap(); assert!(w.updates.is_empty()); assert!(w.sleeps.is_empty());
}
'''.replace("@LIFECYCLE@", lifecycle)
    with tempfile.TemporaryDirectory(prefix="games-native-audio-") as directory:
        directory = Path(directory)
        (directory / "tests.rs").write_text(harness)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(directory / "tests.rs"), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
