use std::{fs, path::Path};

fn read(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_else(|error| {
        panic!("failed to read {relative}: {error}");
    })
}

#[test]
fn trueos_mio_uses_the_unix_poll_selector_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sys = read(root, "vendor/mio-1.2.0/src/sys/mod.rs");
    let unix = read(root, "vendor/mio-1.2.0/src/sys/unix/mod.rs");
    let poll = read(root, "vendor/mio-1.2.0/src/sys/unix/selector/poll.rs");

    assert!(
        sys.contains("#[cfg(any(\n    unix,\n    target_os = \"hermit\"")
            && !sys.contains("mod trueos;"),
        "TRUEOS must stay on Mio's Unix selector instead of a second native reactor"
    );
    assert!(
        unix.contains("target_os = \"trueos\"")
            && unix.contains("path = \"selector/poll.rs\"")
            && unix.contains("pub(crate) use self::selector::*;"),
        "TRUEOS must compile Mio's poll(2) selector"
    );
    assert!(
        unix.contains("path = \"waker/pipe.rs\"") && unix.contains("target_os = \"trueos\""),
        "TRUEOS Mio must keep the pipe waker in the same poll set"
    );
    assert!(
        poll.contains("RegistrationMode::Persistent")
            && poll.contains("RegistrationMode::ManagedOneShot"),
        "raw SourceFd registrations and Mio-managed IoSource registrations need distinct rearm semantics"
    );
    assert!(
        !root.join("vendor/mio-1.2.0/src/sys/trueos").exists(),
        "do not reintroduce a duplicate TRUEOS selector registry"
    );
}
