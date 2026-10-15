mod socket_checks;
mod websocket_checks;

use std::time::Instant;

pub(crate) fn record(state: &str, case: &str, detail: &str) {
    let line = format!("network_contract: {state} case={case} {detail}");
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, format_args!("{line}"));
    #[cfg(not(target_os = "trueos"))]
    println!("{line}");
}

fn main() {
    let mut host = String::from("192.168.178.111");
    let mut base_port = 32350u16;
    let mut runtime_mode = String::from("both");
    let args: Vec<String> = std::env::args().skip(1).collect();
    for arg in args {
        if let Some(value) = arg.strip_prefix("--host=") {
            host = value.into();
        } else if let Some(value) = arg.strip_prefix("--port=") {
            base_port = match value.parse::<u16>() {
                Ok(port @ 1..=65532) => port,
                _ => {
                    record("FAIL", "arguments", "base port must be between 1 and 65532");
                    std::process::exit(2);
                }
            };
        } else if let Some(value) = arg.strip_prefix("--runtime=") {
            runtime_mode = value.into();
        } else {
            record("FAIL", "arguments", &format!("unknown argument {arg}"));
            std::process::exit(2);
        }
    }
    if !matches!(runtime_mode.as_str(), "both" | "current" | "multi") {
        record(
            "FAIL",
            "arguments",
            "runtime must be current, multi or both",
        );
        std::process::exit(2);
    }
    let started = Instant::now();
    record(
        "BEGIN",
        "suite",
        &format!("host={host} port={base_port} runtime={runtime_mode} protocol=1"),
    );
    let mut passed = 0usize;
    let mut failed = 0usize;
    record("BEGIN", "socket.contracts", "backend=std-posix");
    for (case, result) in socket_checks::run(&format!("{host}:{base_port}")) {
        report(case, result, &mut passed, &mut failed);
    }
    let _ = rustls_rustcrypto::provider().install_default();
    for mode in ["current", "multi"] {
        if runtime_mode != "both" && runtime_mode != mode {
            continue;
        }
        record("BEGIN", &format!("runtime.{mode}.build"), "");
        let mut builder = if mode == "current" {
            tokio::runtime::Builder::new_current_thread()
        } else {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            builder.worker_threads(2);
            builder
        };
        let runtime = match builder.enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                report(
                    &format!("runtime.{mode}"),
                    Err(error.to_string()),
                    &mut passed,
                    &mut failed,
                );
                continue;
            }
        };
        runtime.block_on(async {
            websocket_checks::run(&host, base_port, mode, &mut passed, &mut failed).await;
        });
        drop(runtime);
        record("PASS", &format!("runtime.{mode}.drop"), "joined");
    }
    record(
        if failed == 0 { "PASS" } else { "FAIL" },
        "suite",
        &format!(
            "passed={passed} failed={failed} elapsed_ms={}",
            started.elapsed().as_millis()
        ),
    );
    if failed != 0 {
        std::process::exit(1);
    }
}

pub(crate) fn report(
    case: &str,
    result: Result<(), String>,
    passed: &mut usize,
    failed: &mut usize,
) {
    match result {
        Ok(()) => {
            *passed += 1;
            record("PASS", case, "");
        }
        Err(error) => {
            *failed += 1;
            record("FAIL", case, &error);
        }
    }
}
