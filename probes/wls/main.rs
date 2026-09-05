use std::cell::Cell;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::Duration;
use trueos::{logl::{self, level}, t};

thread_local! {
    static MARKER: Cell<u64> = const { Cell::new(0) };
}
const MAIN_MARK: u64 = 0xA11C_E001;
const DEADLINE: Duration = Duration::from_secs(5);

fn main() {
    let runtime = match t::runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(level::ERROR, format_args!("wls: FAIL runtime={error}"));
            return;
        }
    };
    let result = runtime.block_on(run_probe());
    match result {
        Ok(()) => logl::log(level::INFO, format_args!("wls: PASS native.isolation runtime.reuse std.spawn.unsupported")),
        Err(stage) => logl::log(level::ERROR, format_args!("wls: FAIL stage={stage}")),
    }
}

async fn run_probe() -> Result<(), &'static str> {
    match std::thread::Builder::new().spawn(|| ()) {
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {}
        _ => return Err("std.spawn.must-be-unsupported"),
    }
    if t::worker::capacity() < 2 { return Err("insufficient-native-capacity"); }
    MARKER.with(|marker| marker.set(MAIN_MARK));
    let main_slot = t::worker::local_slot();
    let ready = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let mut jobs = Vec::new();
    let mut error = None;
    for index in 0..2u64 {
        let ready = ready.clone();
        let release = release.clone();
        let job = t::worker::spawn(move || {
            let slot = t::worker::local_slot();
            let mark = 0xB10C_0000 | index;
            // Sequential jobs may reuse this slot. Record, don't require zero.
            let previous = MARKER.with(|marker| { let value = marker.get(); marker.set(mark); value });
            for iteration in 0..2 {
                let runtime = t::runtime::current_thread().build().map_err(|_| "worker.runtime")?;
                let result = runtime.block_on(async {
                    if iteration == 0 {
                        ready.fetch_add(1, Ordering::AcqRel);
                        t::time::timeout(DEADLINE, async {
                            while !release.load(Ordering::Acquire) {
                                t::time::sleep(Duration::from_millis(1)).await;
                            }
                        }).await.map_err(|_| "worker.release.timeout")?;
                    }
                    t::task::yield_now().await;
                    t::time::sleep(Duration::from_millis(2)).await;
                    if MARKER.with(Cell::get) != mark || t::worker::local_slot() != slot {
                        return Err("worker.isolation");
                    }
                    Ok(())
                });
                drop(runtime); // test re-entering std/Tokio state on this same WLS slot
                result?;
            }
            Ok::<_, &'static str>((slot, previous, mark))
        });
        match job {
            Ok(job) => jobs.push(job),
            Err(_) => { error = Some("worker.submit"); break; }
        }
    }
    if error.is_none() && t::time::timeout(DEADLINE, async {
        while ready.load(Ordering::Acquire) != 2 { t::time::sleep(Duration::from_millis(1)).await; }
    }).await.is_err() { error = Some("coordinator.ready.timeout"); }
    release.store(true, Ordering::Release);
    let mut slots = Vec::new();
    for mut job in jobs {
        // A timeout is reported, but never turns a live job into a successful
        // cancellation. Continue draining while the kernel retains its code.
        let joined = match t::time::timeout(DEADLINE, &mut job).await {
            Ok(result) => result,
            Err(_) => { error.get_or_insert("worker.join.timeout"); job.await }
        };
        match joined {
            Ok(Ok((slot, previous, mark))) => {
                logl::log(level::INFO, format_args!("wls: slot={slot} previous={previous:x} mark={mark:x} runtime_cycles=2"));
                slots.push(slot);
            }
            Ok(Err(stage)) => { error.get_or_insert(stage); }
            Err(_) => { error.get_or_insert("worker.join"); }
        }
    }
    if slots.len() != 2 || slots[0] == slots[1] || slots.contains(&main_slot) {
        error.get_or_insert("distinct-worker-slots");
    }
    if MARKER.with(Cell::get) != MAIN_MARK { error.get_or_insert("coordinator.isolation"); }
    error.map_or(Ok(()), Err)
}
