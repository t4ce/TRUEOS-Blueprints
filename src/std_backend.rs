//! Install the canonical backend from the selected kernel checkout at pack time.

use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

pub(crate) fn install(blueprint_root: &Path) -> Result<(), String> {
    let kernel = crate::abi_guard::locate_kernel_repo(blueprint_root)?
        .ok_or("TRUEOS std backend requires a sibling TRUEOS checkout or TRUEOS_REPO_ROOT")?;
    let source_root = crate::toolchain::rust_sysroot()?.join("lib/rustlib/src/rust");
    let contract_check = kernel.join("tools/testpy/check_native_worker_contract.py");
    let status = Command::new("python3")
        .arg(&contract_check)
        .arg("--kernel")
        .arg(&kernel)
        .arg("--blueprints")
        .arg(blueprint_root)
        .status()
        .map_err(|error| format!("cannot run {}: {error}", contract_check.display()))?;
    if !status.success() {
        return Err("native worker source ABI verification failed".into());
    }
    let installer = kernel.join("tools/testpy/apply_trueos_rust_std_thread_backend.py");
    let output = Command::new("python3")
        .arg(&installer)
        .arg(&source_root)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", installer.display()))?;
    if !output.status.success() {
        return Err(format!(
            "TRUEOS std backend installation failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

pub(crate) fn cache_revision(target_spec: &Path) -> Result<String, String> {
    let library = crate::toolchain::rust_sysroot()?.join("lib/rustlib/src/rust/library");
    let mut hash = Sha256::new();
    // Include the installed thread lifecycle, key-backed TLS storage and
    // destructor hooks: rust-src is outside Cargo's dependency tracking.
    for relative in [
        "std/src/sys/thread/mod.rs",
        "std/src/sys/thread/trueos.rs",
        "std/src/os/unix/mod.rs",
        "std/src/thread/current.rs",
        "std/src/sys/thread_local/mod.rs",
        "std/src/sys/thread_local/os.rs",
        "std/src/sys/thread_local/key/unix.rs",
        "std/src/sys/thread_local/key/racy.rs",
        "std/src/sys/thread_local/guard/key.rs",
        "std/src/hash/random.rs",
        "std/src/sys/pal/unix/time.rs",
    ] {
        let source = fs::read(library.join(relative))
            .map_err(|error| format!("cannot fingerprint rust-src {relative}: {error}"))?;
        hash.update(relative.as_bytes());
        hash.update((source.len() as u64).to_le_bytes());
        hash.update(source);
    }
    // Custom target JSON lives outside the rust-src dependency graph too.
    // In particular, guarded stacks require every crate, including std, to
    // be rebuilt when inline stack probes are enabled.
    let target = fs::read(target_spec)
        .map_err(|error| format!("cannot fingerprint target {}: {error}", target_spec.display()))?;
    hash.update(b"target-spec");
    hash.update((target.len() as u64).to_le_bytes());
    hash.update(target);
    Ok(format!("{:x}", hash.finalize())[..16].to_owned())
}
