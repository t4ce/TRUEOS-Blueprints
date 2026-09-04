use std::{env, fs, path::PathBuf, process::Command};

const TRUEOS_NO_DETACH_MARKER: &str =
    "TRUEOS std thread wrappers do not own a native pthread lifecycle";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let Ok(output) = Command::new(rustc).args(["--print", "sysroot"]).output() else {
        println!("cargo:warning=TRUEOS rust-src detach patch skipped: failed to run rustc");
        return;
    };
    if !output.status.success() {
        println!("cargo:warning=TRUEOS rust-src detach patch skipped: rustc --print sysroot failed");
        return;
    }

    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let unix_thread = PathBuf::from(sysroot)
        .join("lib/rustlib/src/rust/library/std/src/sys/thread/unix.rs");
    println!("cargo:rerun-if-changed={}", unix_thread.display());

    let Ok(source) = fs::read_to_string(&unix_thread) else {
        // The Blueprint packer already reports a precise rust-src error when a
        // Tokio/std Blueprint is actually built. Do not make building the host
        // packer itself require the rust-src component.
        return;
    };
    if source.contains(TRUEOS_NO_DETACH_MARKER) {
        return;
    }

    let needle = r#"impl Drop for Thread {
    fn drop(&mut self) {
        let ret = unsafe { libc::pthread_detach(self.id) };
        debug_assert_eq!(ret, 0);
    }
}"#;
    let replacement = r#"impl Drop for Thread {
    fn drop(&mut self) {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            // TRUEOS std thread wrappers do not own a native pthread lifecycle.
            // In particular, Tokio's current-thread runtime may contain dormant
            // blocking-pool JoinHandle types without ever creating a thread.
            // Dropping those types must therefore not manufacture a
            // `pthread_detach` import.
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            let ret = unsafe { libc::pthread_detach(self.id) };
            debug_assert_eq!(ret, 0);
        }
    }
}"#;

    if !source.contains(needle) {
        panic!(
            "failed to patch {}; missing Rust std Thread::drop pthread_detach marker",
            unix_thread.display()
        );
    }

    let patched = source.replacen(needle, replacement, 1);
    fs::write(&unix_thread, patched).unwrap_or_else(|err| {
        panic!(
            "failed to patch Rust std thread source {}: {err}",
            unix_thread.display()
        )
    });

    println!(
        "cargo:warning=patched TRUEOS Rust std Thread::drop to omit pthread_detach"
    );
}
