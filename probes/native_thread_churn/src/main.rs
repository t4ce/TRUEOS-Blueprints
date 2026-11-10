//! Bounded, serial native thread lifetime probe. See ../BUILDING.txt.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

const ITERATIONS: u32 = 200;
const RETIRE_DELAY: Duration = Duration::from_millis(1);

#[cfg(target_os = "trueos")]
const BACKEND: &str = "trueos";
#[cfg(not(target_os = "trueos"))]
const BACKEND: &str = "host-model";

fn checksum(iteration: u32) -> u64 {
    let mut value = u64::from(iteration);
    for _ in 0..64 {
        value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
        value = std::hint::black_box(value);
    }
    value
}

fn boundary(
    output: &mut impl Write,
    stage: &str,
    attempted: u32,
    completed: u32,
    stack_bytes: usize,
) -> io::Result<()> {
    if matches!(attempted, 1 | 2 | 16 | 112 | 120 | 128) {
        writeln!(
            output,
            "thread-churn v=1 backend={BACKEND} stage={stage} attempted={attempted} completed={completed} stack_bytes={stack_bytes}"
        )?;
        output.flush()?;
    }
    Ok(())
}

fn run(output: &mut impl Write, stack_bytes: usize) -> io::Result<()> {
    writeln!(
        output,
        "thread-churn v=1 backend={BACKEND} stage=begin iterations={ITERATIONS} stack_bytes={stack_bytes} peak_threads=2 retire_delay_ms=1"
    )?;
    output.flush()?;

    let mut digest = 0u64;
    for iteration in 1..=ITERATIONS {
        boundary(
            output,
            "before_spawn",
            iteration,
            iteration - 1,
            stack_bytes,
        )?;
        // Join the sole child before creating the next one. This measures
        // accumulated thread lifetimes rather than concurrent thread capacity.
        let worker = match thread::Builder::new()
            .name("churn-worker".into())
            .stack_size(stack_bytes)
            .spawn(move || checksum(iteration))
        {
            Ok(worker) => worker,
            Err(error) => {
                writeln!(
                    output,
                    "thread-churn v=1 backend={BACKEND} stage=spawn_failed pass=0 attempted={iteration} completed={} stack_bytes={stack_bytes} raw_errno={}",
                    iteration - 1,
                    error.raw_os_error().unwrap_or(-1)
                )?;
                return output.flush();
            }
        };
        boundary(output, "after_spawn", iteration, iteration - 1, stack_bytes)?;
        let observed = match worker.join() {
            Ok(observed) => observed,
            Err(_) => {
                writeln!(
                    output,
                    "thread-churn v=1 backend={BACKEND} stage=join_failed pass=0 attempted={iteration} completed={} stack_bytes={stack_bytes}",
                    iteration - 1
                )?;
                return output.flush();
            }
        };
        boundary(output, "after_join", iteration, iteration, stack_bytes)?;
        if observed != checksum(iteration) {
            writeln!(
                output,
                "thread-churn v=1 backend={BACKEND} stage=work_failed pass=0 attempted={iteration} completed={} stack_bytes={stack_bytes}",
                iteration - 1
            )?;
            return output.flush();
        }
        digest = digest.wrapping_add(observed);
        // Give the native carrier a suspension boundary to retire the previous
        // continuation and its backing before the next stack is requested.
        thread::sleep(RETIRE_DELAY);
        boundary(output, "after_retire", iteration, iteration, stack_bytes)?;
        if iteration % 16 == 0 {
            writeln!(
                output,
                "thread-churn v=1 backend={BACKEND} stage=progress completed={iteration} stack_bytes={stack_bytes}"
            )?;
            output.flush()?;
        }
    }
    writeln!(
        output,
        "thread-churn v=1 backend={BACKEND} stage=done pass=1 completed={ITERATIONS} stack_bytes={stack_bytes} digest={digest}"
    )?;
    output.flush()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let stack_bytes = match (args.next().as_deref(), args.next()) {
        (None | Some("16m"), None) => 16 * 1024 * 1024,
        (Some("2m"), None) => 2 * 1024 * 1024,
        _ => {
            let _ = writeln!(
                io::stdout(),
                "thread-churn v=1 backend={BACKEND} stage=invalid_args pass=0 usage=thread-churn_[16m|2m]"
            );
            return;
        }
    };
    let mut output = io::stdout().lock();
    if let Err(error) = run(&mut output, stack_bytes) {
        // Never turn a fallible spawn/output failure into a probe panic, and
        // emit only fixed markers and numeric errors, not paths or host data.
        let _ = writeln!(
            io::stderr(),
            "thread-churn v=1 backend={BACKEND} stage=output_failed pass=0 raw_errno={}",
            error.raw_os_error().unwrap_or(-1)
        );
    }
    // Shell3 renders console frames asynchronously. Leave the final flushed
    // numeric result visible before VM teardown retires the console backend.
    thread::sleep(Duration::from_millis(250));
}
