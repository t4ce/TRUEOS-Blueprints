//! Unlock related source-overlay changes in one Cargo resolution.
//!
//! New overlay features can make every individual `cargo update --precise`
//! fail while another affected package remains locked to its old release.
//! A multi-package update releases those old decisions together. The caller
//! still runs its existing precise alignment and final lock verification.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::{CratePatch, LockMismatch, LockedPackage};

fn package_spec(package: &LockedPackage) -> String {
    let name_version = format!("{}@{}", package.name, package.version);
    // Full registry package IDs also disambiguate equal name/version pairs
    // from another registry or Git source. Path lock entries do not retain
    // their manifest location; keep their version qualifier.
    match package.source.as_deref() {
        Some(source) if source.starts_with("registry+") => format!("{source}#{name_version}"),
        _ => name_version,
    }
}

fn resolver_conflict_name(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr).lines().find_map(|line| {
        let rest = line.strip_prefix("error: failed to select a version for `")?;
        let (name, ending) = rest.split_once('`')?;
        (!name.is_empty() && ending == ".").then(|| name.to_owned())
    })
}

pub(crate) fn unlock_staged_lock_overlay_batch(
    staged_manifest: &Path,
    staged_source_overlay: &[CratePatch],
    mismatches: &[LockMismatch],
) -> Result<(), String> {
    // Preserve the established single-package update path, including its
    // futures-family ordering and version-qualified fallback behavior.
    if mismatches.len() <= 1 {
        return Ok(());
    }
    let locked = crate::lock_packages(&crate::generated_lock_path(staged_manifest)?)?;
    let mut specs = BTreeSet::new();
    for mismatch in mismatches {
        if crate::staged_lock_overlay_aligned(staged_manifest, mismatch)? {
            continue;
        }
        for package in locked.iter().filter(|package| {
            package.name == mismatch.name
                && package.version != mismatch.overlay_version
                && crate::cargo_semver_same_line(&package.version, &mismatch.overlay_version)
        }) {
            specs.insert(package_spec(package));
        }
    }
    if specs.len() <= 1 {
        return Ok(());
    }

    // Resolve configuration beside the staged manifest. Absolutize patch paths
    // first so changing the command directory preserves their original meaning.
    let staged_manifest = fs::canonicalize(staged_manifest).map_err(crate::io_string)?;
    let working_directory = std::env::current_dir().map_err(crate::io_string)?;
    let patches: Vec<_> = staged_source_overlay
        .iter()
        .map(|patch| CratePatch {
            key: patch.key.clone(),
            name: patch.name.clone(),
            path: if patch.path.is_absolute() {
                patch.path.clone()
            } else {
                working_directory.join(&patch.path)
            },
        })
        .collect();

    loop {
        println!(
            "trueos-blueprint: unlocking staged lock overlays together: {}",
            specs
                .iter()
                .map(|spec| spec.rsplit('#').next().unwrap_or(spec))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut update = crate::toolchain::cargo_command();
        update
            .arg("update")
            .arg("--manifest-path")
            .arg(&staged_manifest)
            .current_dir(
                staged_manifest
                    .parent()
                    .ok_or("staged manifest has no directory")?,
            );
        for spec in &specs {
            update.arg("-p").arg(spec);
        }
        // Cargo accepts --precise for one package only. The checked-in path
        // overlays remain the candidate sources during this joint resolution.
        crate::push_source_overlay_configs(&mut update, &patches);
        let output = update.output().map_err(|failure| {
            format!("batch staged lock overlay update failed to start: {failure}")
        })?;
        let notes =
            crate::write_filtered_cargo_output("cargo update", &output.stdout, &output.stderr)?;
        crate::print_cargo_output_notes("cargo update", &notes);
        if output.status.success() {
            return Ok(());
        }

        // A source overlay can raise a dependency requirement without changing
        // its own version. Unlock only the package Cargo actually named, using
        // existing staged lock entries rather than trusting stderr as a spec.
        // Every retry must add a new locked spec, so an unresolved repeated
        // conflict or an unknown package terminates instead of looping.
        let previous_count = specs.len();
        if let Some(name) = resolver_conflict_name(&output.stderr) {
            let current = crate::lock_packages(&crate::generated_lock_path(&staged_manifest)?)?;
            for package in current.iter().filter(|package| package.name == name) {
                specs.insert(package_spec(package));
            }
            if specs.len() > previous_count {
                println!(
                    "trueos-blueprint: also unlocking Cargo-reported dependency conflict: {name}"
                );
                continue;
            }
        }
        return Err(format!(
            "batch staged lock overlay update failed with status {}",
            output.status
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Output;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        patches: Vec<CratePatch>,
        mismatches: Vec<LockMismatch>,
        original_lock: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Self {
            Self::create(false)
        }

        fn create(shared_conflict: bool) -> Self {
            let root = std::env::temp_dir().join(format!(
                "blueprint-atomic-overlays-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            crate::reset_dir(&root).unwrap();
            fs::create_dir_all(root.join(".cargo")).unwrap();
            fs::write(root.join(".cargo/config.toml"), format!(
                "[net]\noffline = true\n[source.crates-io]\nreplace-with = \"fixture\"\n[source.fixture]\ndirectory = {}\n",
                crate::toml_string(&root.join("registry").to_string_lossy())
            )).unwrap();
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
            fs::write(root.join("Cargo.toml"), concat!(
                "[package]\nname = \"bp-batch-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                "[workspace]\n[dependencies]\nbp-batch-consumer-a = \"1\"\nbp-batch-consumer-b = \"1\"\n"
            )).unwrap();

            let mut patches = Vec::new();
            let mut mismatches = Vec::new();
            for suffix in ["a", "b"] {
                let provider = format!("bp-batch-provider-{suffix}");
                let consumer = format!("bp-batch-consumer-{suffix}");
                write_crate(&root, "registry", &provider, "1.0.0", "[features]\n");
                write_crate(
                    &root,
                    "registry",
                    &consumer,
                    "1.0.0",
                    &format!("[dependencies]\n{provider} = \"1\"\n"),
                );
                let provider_patch = write_crate(
                    &root,
                    "patches",
                    &provider,
                    "1.1.0",
                    "[features]\nnew = []\n",
                );
                let consumer_patch = write_crate(
                    &root,
                    "patches",
                    &consumer,
                    "1.0.0",
                    &format!(
                        "[dependencies]\n{provider} = {{ version = \"1\", features = [\"new\"] }}\n"
                    ),
                );
                patches.push(CratePatch::new(&provider, provider_patch));
                patches.push(CratePatch::new(&consumer, consumer_patch));
                mismatches.push(LockMismatch {
                    name: provider,
                    locked_version: "1.0.0".to_owned(),
                    overlay_version: "1.1.0".to_owned(),
                });
            }
            if shared_conflict {
                write_crate(&root, "registry", "bp-batch-shared", "1.0.0", "");
                write_crate(&root, "registry", "bp-batch-pinned", "1.0.0", "");
                write_crate(
                    &root,
                    "registry",
                    "bp-batch-raised-consumer",
                    "1.0.0",
                    "[dependencies]\nbp-batch-shared = \"1\"\n",
                );
                write_crate(
                    &root,
                    "registry",
                    "bp-batch-unchanged-consumer",
                    "1.0.0",
                    "[dependencies]\nbp-batch-shared = \"1\"\nbp-batch-pinned = \"1\"\n",
                );
                let raised = write_crate(
                    &root,
                    "patches",
                    "bp-batch-raised-consumer",
                    "1.0.0",
                    "[dependencies]\nbp-batch-shared = \"1.1\"\n",
                );
                patches.push(CratePatch::new("bp-batch-raised-consumer", raised));
                use std::io::Write;
                let mut manifest = fs::OpenOptions::new()
                    .append(true)
                    .open(root.join("Cargo.toml"))
                    .unwrap();
                manifest
                    .write_all(
                        b"bp-batch-raised-consumer = \"1\"\nbp-batch-unchanged-consumer = \"1\"\n",
                    )
                    .unwrap();
            }
            let output = crate::toolchain::cargo_command()
                .current_dir(&root)
                .args(["generate-lockfile", "--offline"])
                .output()
                .unwrap();
            assert_success(&output);
            let original_lock = fs::read(root.join("Cargo.lock")).unwrap();
            // Publish newer transitive releases only after the initial pins
            // exist. Neither provider is itself an overlay/version mismatch.
            if shared_conflict {
                write_crate(&root, "registry", "bp-batch-shared", "1.1.0", "");
                write_crate(&root, "registry", "bp-batch-pinned", "1.1.0", "");
            }
            Self {
                root,
                patches,
                mismatches,
                original_lock,
            }
        }

        fn manifest(&self) -> PathBuf {
            self.root.join("Cargo.toml")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_crate(
        root: &Path,
        source: &str,
        name: &str,
        version: &str,
        declarations: &str,
    ) -> PathBuf {
        let directory = root.join(source).join(format!("{name}-{version}"));
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::write(directory.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
        fs::write(directory.join("Cargo.toml"), format!(
            "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2024\"\n{declarations}"
        )).unwrap();
        if source == "registry" {
            fs::write(
                directory.join(".cargo-checksum.json"),
                "{\"files\":{},\"package\":null}",
            )
            .unwrap();
        }
        directory
    }

    fn assert_success(output: &Output) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn atomic_update_resolves_two_feature_conflicts_that_block_either_sequential_order() {
        let fixture = Fixture::new();
        for mismatch in &fixture.mismatches {
            fs::write(fixture.root.join("Cargo.lock"), &fixture.original_lock).unwrap();
            let mut update = crate::toolchain::cargo_command();
            update
                .current_dir(&fixture.root)
                .args(["update", "--offline", "-p"])
                .arg(format!("{}@{}", mismatch.name, mismatch.locked_version))
                .args(["--precise", &mismatch.overlay_version]);
            crate::push_source_overlay_configs(&mut update, &fixture.patches);
            let output = update.output().unwrap();
            assert!(
                !output.status.success(),
                "individual update unexpectedly resolved both conflicts"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("feature `new`") && stderr.contains("does not have that feature"),
                "{stderr}"
            );
            assert_eq!(
                fs::read(fixture.root.join("Cargo.lock")).unwrap(),
                fixture.original_lock,
                "failed resolution must leave the staged lock unchanged"
            );
        }

        unlock_staged_lock_overlay_batch(
            &fixture.manifest(),
            &fixture.patches,
            &fixture.mismatches,
        )
        .unwrap();
        for mismatch in &fixture.mismatches {
            assert!(crate::staged_lock_overlay_aligned(&fixture.manifest(), mismatch).unwrap());
        }
        let resolved = fs::read(fixture.root.join("Cargo.lock")).unwrap();
        assert_ne!(resolved, fixture.original_lock);
        unlock_staged_lock_overlay_batch(
            &fixture.manifest(),
            &fixture.patches,
            &fixture.mismatches,
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.root.join("Cargo.lock")).unwrap(),
            resolved,
            "already aligned staged locks must remain untouched"
        );
    }

    #[test]
    fn single_mismatch_retains_the_existing_precise_update_path() {
        let fixture = Fixture::new();
        unlock_staged_lock_overlay_batch(
            &fixture.manifest(),
            &fixture.patches,
            &fixture.mismatches[..1],
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.root.join("Cargo.lock")).unwrap(),
            fixture.original_lock
        );
    }

    #[test]
    fn reported_transitive_conflict_is_unlocked_without_updating_unrelated_pins() {
        let fixture = Fixture::create(true);
        // The initial atomic set still fails: the same-version consumer
        // overlay raises `shared`, but its unchanged second consumer retains
        // the old provider pin. This dependency has no overlay of its own.
        let mut initial = crate::toolchain::cargo_command();
        initial
            .current_dir(&fixture.root)
            .args(["update", "--offline"]);
        for mismatch in &fixture.mismatches {
            initial
                .arg("-p")
                .arg(format!("{}@{}", mismatch.name, mismatch.locked_version));
        }
        crate::push_source_overlay_configs(&mut initial, &fixture.patches);
        let output = initial.output().unwrap();
        assert!(!output.status.success());
        assert_eq!(
            resolver_conflict_name(&output.stderr).as_deref(),
            Some("bp-batch-shared"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(fixture.root.join("Cargo.lock")).unwrap(),
            fixture.original_lock
        );

        unlock_staged_lock_overlay_batch(
            &fixture.manifest(),
            &fixture.patches,
            &fixture.mismatches,
        )
        .unwrap();
        let locked = crate::lock_packages(&fixture.root.join("Cargo.lock")).unwrap();
        assert!(
            locked
                .iter()
                .any(|package| package.name == "bp-batch-shared" && package.version == "1.1.0")
        );
        assert!(
            locked
                .iter()
                .any(|package| package.name == "bp-batch-pinned" && package.version == "1.0.0"),
            "an unrelated available update must remain pinned"
        );
        for mismatch in &fixture.mismatches {
            assert!(crate::staged_lock_overlay_aligned(&fixture.manifest(), mismatch).unwrap());
        }
    }

    #[test]
    fn resolver_name_parser_accepts_only_the_named_cargo_selection_error() {
        assert_eq!(
            resolver_conflict_name(b"error: failed to select a version for `either`.\n").as_deref(),
            Some("either")
        );
        assert_eq!(
            resolver_conflict_name(b"error: failed to select a version for ``.\n"),
            None
        );
        assert_eq!(
            resolver_conflict_name(
                b"error: failed to select a version for `either` trailing arguments\n"
            ),
            None
        );
        assert_eq!(
            resolver_conflict_name(b"an unrelated error mentioning `either`\n"),
            None
        );
    }
}
