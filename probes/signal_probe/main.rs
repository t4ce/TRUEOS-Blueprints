use core::ffi::c_int;

use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::time::{Duration, timeout};
use trueos::{logl, logl::level};

const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;

unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
    fn raise(signal: c_int) -> c_int;
}

fn errno() -> c_int {
    unsafe { *__errno_location() }
}

fn register(name: &'static str, kind: SignalKind) -> Result<Signal, &'static str> {
    logl::log(
        level::INFO,
        format_args!("signal-probe: stage register.{name}"),
    );
    match signal(kind) {
        Ok(stream) => {
            logl::log(
                level::INFO,
                format_args!("signal-probe: success register.{name}"),
            );
            Ok(stream)
        }
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!(
                    "signal-probe: failed register.{name} error={error} raw_errno={:?}",
                    error.raw_os_error()
                ),
            );
            Err("register")
        }
    }
}

async fn deliver_and_receive(
    name: &'static str,
    signal_number: c_int,
    stream: &mut Signal,
) -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!(
            "signal-probe: stage raise.{name} owner=current-blueprint signal={signal_number}"
        ),
    );
    let rc = unsafe { raise(signal_number) };
    if rc != 0 {
        logl::log(
            level::ERROR,
            format_args!(
                "signal-probe: failed raise.{name} rc={rc} errno={}",
                errno()
            ),
        );
        return Err("raise");
    }

    match timeout(Duration::from_secs(2), stream.recv()).await {
        Ok(Some(())) => {
            logl::log(
                level::INFO,
                format_args!("signal-probe: success receive.{name}"),
            );
            Ok(())
        }
        Ok(None) => {
            logl::log(
                level::ERROR,
                format_args!("signal-probe: failed receive.{name} stream_closed=true"),
            );
            Err("stream_closed")
        }
        Err(_) => {
            logl::log(
                level::ERROR,
                format_args!("signal-probe: failed receive.{name} timeout_ms=2000"),
            );
            Err("receive_timeout")
        }
    }
}

async fn run_probe() -> Result<(), &'static str> {
    // Register all three before waiting. This both proves independent action
    // slots and matches the server behavior we need: INT, HUP, and TERM must
    // all be armed concurrently.
    let mut interrupt = register("sigint", SignalKind::interrupt())?;
    let mut hangup = register("sighup", SignalKind::hangup())?;
    let mut terminate = register("sigterm", SignalKind::terminate())?;

    deliver_and_receive("sigint", SIGINT, &mut interrupt).await?;
    deliver_and_receive("sighup", SIGHUP, &mut hangup).await?;
    deliver_and_receive("sigterm", SIGTERM, &mut terminate).await?;
    Ok(())
}

fn main() {
    logl::log(level::INFO, "signal-probe: blueprint start");

    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_all();
    let runtime = match builder.build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("signal-probe: runtime build failed: {error}"),
            );
            return;
        }
    };

    match runtime.block_on(run_probe()) {
        Ok(()) => logl::log(level::INFO, "signal-probe: PASS unix-tokio-signal-stack"),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("signal-probe: FAIL stage={stage}"),
        ),
    }
}
