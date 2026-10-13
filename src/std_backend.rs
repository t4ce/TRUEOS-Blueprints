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

const CACHE_SOURCES: &[&str] = &[
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
    "std/src/sys/fs/unix.rs",
];

pub(crate) fn cache_revision(target_spec: &Path) -> Result<String, String> {
    let library = crate::toolchain::rust_sysroot()?.join("lib/rustlib/src/rust/library");
    cache_revision_for_library(&library, target_spec)
}

fn cache_revision_for_library(library: &Path, target_spec: &Path) -> Result<String, String> {
    let mut hash = Sha256::new();
    // Patched rust-src is outside Cargo's dependency tracking. Include the
    // filesystem backend as well as the thread, TLS, random and time hooks.
    for relative in CACHE_SOURCES {
        let source = fs::read(library.join(relative))
            .map_err(|error| format!("cannot fingerprint rust-src {relative}: {error}"))?;
        hash.update(relative.as_bytes());
        hash.update((source.len() as u64).to_le_bytes());
        hash.update(source);
    }
    // Custom target JSON lives outside the rust-src dependency graph too.
    // In particular, guarded stacks require every crate, including std, to
    // be rebuilt when inline stack probes are enabled.
    let target = fs::read(target_spec).map_err(|error| {
        format!(
            "cannot fingerprint target {}: {error}",
            target_spec.display()
        )
    })?;
    hash.update(b"target-spec");
    hash.update((target.len() as u64).to_le_bytes());
    hash.update(target);
    Ok(format!("{:x}", hash.finalize())[..16].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_backend_changes_invalidate_std_cache() {
        let root = std::env::temp_dir().join(format!("trueos-std-cache-fs-{}", std::process::id()));
        for relative in CACHE_SOURCES {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"fixture source").unwrap();
        }
        let target = root.join("target.json");
        fs::write(&target, b"{}").unwrap();
        let before = cache_revision_for_library(&root, &target).unwrap();
        fs::write(
            root.join("std/src/sys/fs/unix.rs"),
            b"typed directory decoder",
        )
        .unwrap();
        let after = cache_revision_for_library(&root, &target).unwrap();
        assert_ne!(before, after);
        assert_eq!(after, cache_revision_for_library(&root, &target).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
