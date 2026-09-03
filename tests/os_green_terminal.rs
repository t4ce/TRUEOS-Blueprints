use std::{fs, path::Path};

fn read(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_else(|error| {
        panic!("failed to read {relative}: {error}");
    })
}

#[test]
fn os_terminal_loop_stays_cooperative() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = read(root, "buildins/os/Cargo.toml");
    let source = read(root, "buildins/os/src/main.rs");

    assert!(
        manifest.contains("features = [\"tokio-runtime\"]"),
        "os must build with TRUEOS's current-thread Tokio surface"
    );
    assert!(
        source.contains("runtime::current_thread().build()")
            && source.contains("task::spawn(async move"),
        "os terminal work must execute as a green Tokio task"
    );
    assert!(
        source.contains("event::poll(Duration::ZERO)?"),
        "os must probe Crossterm without entering the blocking poll path"
    );
    assert!(
        source.contains("platform::poll_once();") && source.contains("task::yield_now().await;"),
        "idle terminal work must yield the guest and the green task without sleeping a vthread"
    );
    assert!(
        !source.contains("event::poll(Duration::from_") && !source.contains("time::sleep("),
        "do not smuggle a timer-backed blocking wait into the cooperative terminal loop"
    );
}
