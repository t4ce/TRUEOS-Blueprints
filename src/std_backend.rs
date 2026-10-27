//! Prepare TRUEOS std through the shared Buildins sysroot installer.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

// Bulk builds share this private source tree. Complete its source adaptations
// before another lane fingerprints or compiles it.
static PREPARE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
pub(crate) struct PreparedSysroot {
    pub(crate) sysroot: PathBuf,
    rustc_wrapper: PathBuf,
}

impl PreparedSysroot {
    pub(crate) fn configure_cargo(&self, command: &mut Command) {
        command.env("RUSTC", &self.rustc_wrapper);
    }

    pub(crate) fn cache_revision(&self, target_spec: &Path) -> Result<String, String> {
        cache_revision_for_library(
            &self.sysroot.join("lib/rustlib/src/rust/library"),
            target_spec,
        )
    }
}

pub(crate) fn prepare(blueprint_root: &Path) -> Result<PreparedSysroot, String> {
    let _guard = PREPARE_LOCK
        .lock()
        .map_err(|_| "std preparation lock poisoned")?;
    let kernel = crate::abi_guard::locate_kernel_repo(blueprint_root)?
        .ok_or("TRUEOS std backend requires a sibling TRUEOS checkout or TRUEOS_REPO_ROOT")?;
    let installer = blueprint_root.join("../TRUEOS-Buildins/tools/prepare_sysroot.py");
    if !installer.is_file() {
        return Err(format!(
            "missing shared std sysroot installer: {}",
            installer.display()
        ));
    }
    let output_dir = blueprint_root.join("target/trueos-blueprint/std");
    fs::create_dir_all(&output_dir).map_err(crate::io_string)?;
    // Serialize supplemental source upgrades across separate packer processes,
    // in addition to the shared installer's own sysroot creation lock.
    let process_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output_dir.join("blueprints.lock"))
        .map_err(crate::io_string)?;
    process_lock.lock().map_err(crate::io_string)?;
    let result_path = output_dir.join("selection.json");
    let installed = crate::toolchain::rust_sysroot()?;
    let output = Command::new("python3")
        .arg(&installer)
        .arg("--output")
        .arg(&output_dir)
        .arg("--kernel")
        .arg(&kernel)
        .arg("--rustc")
        .arg(installed.join("bin/rustc"))
        .arg("--result")
        .arg(&result_path)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", installer.display()))?;
    if !output.status.success() {
        return Err(format!(
            "TRUEOS std sysroot preparation failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    let prepared: PreparedSysroot =
        serde_json::from_slice(&fs::read(&result_path).map_err(crate::io_string)?)
            .map_err(|error| format!("invalid sysroot preparation result: {error}"))?;
    validate_private_sysroot(&installed, &output_dir, &prepared)?;
    crate::ensure_rust_std_trueos_cfg_hooks(&prepared.sysroot)?;
    crate::ensure_rust_std_trueos_hash_random(&prepared.sysroot)?;
    crate::ensure_rust_std_trueos_no_backtrace(&prepared.sysroot)?;

    // The maintained installer verifies its exact backend and thread/TLS
    // selectors. The existing pack-time CABI guard checks linked imports.
    let check = Command::new("python3")
        .arg(kernel.join("tools/testpy/apply_trueos_rust_std_thread_backend.py"))
        .arg("--check")
        .arg(prepared.sysroot.join("lib/rustlib/src/rust"))
        .output()
        .map_err(crate::io_string)?;
    if !check.status.success() {
        return Err(format!(
            "TRUEOS std backend verification failed: {}{}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        ));
    }
    println!(
        "trueos-blueprint: private TRUEOS std sysroot: {}",
        prepared.sysroot.display()
    );
    Ok(prepared)
}

fn validate_private_sysroot(
    installed: &Path,
    output_dir: &Path,
    prepared: &PreparedSysroot,
) -> Result<(), String> {
    let installed_library = installed
        .join("lib/rustlib/src/rust/library")
        .canonicalize()
        .map_err(crate::io_string)?;
    let private_library = prepared
        .sysroot
        .join("lib/rustlib/src/rust/library")
        .canonicalize()
        .map_err(crate::io_string)?;
    let output = output_dir.canonicalize().map_err(crate::io_string)?;
    if private_library == installed_library
        || !private_library.starts_with(output)
        || !prepared.rustc_wrapper.is_file()
    {
        return Err("std installer must return private rust-src and a compiler wrapper".into());
    }
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

    struct FixtureDirectory(PathBuf);

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_sysroots() -> (FixtureDirectory, PathBuf, PathBuf, PreparedSysroot) {
        let root = std::env::temp_dir().join(format!(
            "trueos-private-std-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir(&root).unwrap();
        let temporary = FixtureDirectory(root);
        let installed = temporary.0.join("installed");
        let output = temporary.0.join("output");
        let prepared = PreparedSysroot {
            sysroot: output.join("sysroots/private"),
            rustc_wrapper: output.join("rustc-app"),
        };
        for root in [&installed, &prepared.sysroot] {
            fs::create_dir_all(root.join("lib/rustlib/src/rust/library")).unwrap();
        }
        fs::write(&prepared.rustc_wrapper, b"fixture wrapper").unwrap();
        (temporary, installed, output, prepared)
    }

    #[test]
    #[cfg(unix)]
    fn rejects_installed_sources_disguised_by_a_private_symlink() {
        let (_temporary, installed, output, prepared) = fixture_sysroots();
        let relative = "lib/rustlib/src/rust/library";
        fs::remove_dir(prepared.sysroot.join(relative)).unwrap();
        std::os::unix::fs::symlink(installed.join(relative), prepared.sysroot.join(relative))
            .unwrap();

        let error = validate_private_sysroot(&installed, &output, &prepared).unwrap_err();
        assert!(error.contains("private rust-src"), "{error}");
    }

    #[test]
    fn private_selection_configures_cargo_with_the_selected_wrapper() {
        let (_temporary, installed, output, prepared) = fixture_sysroots();
        validate_private_sysroot(&installed, &output, &prepared).unwrap();
        let mut command = Command::new("cargo");
        command.env("RUSTC", "previous compiler");
        prepared.configure_cargo(&mut command);
        let selected = command
            .get_envs()
            .find(|(name, _)| *name == "RUSTC")
            .and_then(|(_, value)| value);
        assert_eq!(selected, Some(prepared.rustc_wrapper.as_os_str()));

        fs::remove_file(&prepared.rustc_wrapper).unwrap();
        assert!(validate_private_sysroot(&installed, &output, &prepared).is_err());
    }

    #[test]
    fn supplemental_hooks_change_private_sources_and_preserve_installed_sources() {
        let (_temporary, installed, output, prepared) = fixture_sysroots();
        let fixtures = [
            (
                "std/src/hash/random.rs",
                r#"        thread_local!(static KEYS: Cell<(u64, u64)> = {
            Cell::new(hashmap_random_keys())
        });

        KEYS.with(|keys| {
            let (k0, k1) = keys.get();
            keys.set((k0.wrapping_add(1), k1));
            RandomState { k0, k1 }
        })"#,
            ),
            (
                "std/src/sys/backtrace.rs",
                r#"    pub(crate) fn print(&mut self, w: &mut dyn Write, format: PrintFmt) -> io::Result<()> {
        // There are issues currently linking libbacktrace into tests, and in
        write!(w, "{}", DisplayBacktrace { format })
    }
}

/// # Safety"#,
            ),
        ];
        for root in [&installed, &prepared.sysroot] {
            for (relative, contents) in &fixtures {
                let path = root.join("lib/rustlib/src/rust/library").join(relative);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, contents).unwrap();
            }
        }
        validate_private_sysroot(&installed, &output, &prepared).unwrap();
        crate::ensure_rust_std_trueos_hash_random(&prepared.sysroot).unwrap();
        crate::ensure_rust_std_trueos_no_backtrace(&prepared.sysroot).unwrap();

        let mut patched = Vec::new();
        for (relative, original) in &fixtures {
            let path = Path::new("lib/rustlib/src/rust/library").join(relative);
            assert_eq!(
                fs::read_to_string(installed.join(&path)).unwrap(),
                *original
            );
            let source = fs::read_to_string(prepared.sysroot.join(&path)).unwrap();
            assert_ne!(source, *original);
            assert!(source.contains("target_os = \"trueos\""));
            patched.push((path, source));
        }

        crate::ensure_rust_std_trueos_hash_random(&prepared.sysroot).unwrap();
        crate::ensure_rust_std_trueos_no_backtrace(&prepared.sysroot).unwrap();
        for (path, before) in patched {
            assert_eq!(
                fs::read_to_string(prepared.sysroot.join(path)).unwrap(),
                before
            );
        }
    }

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
