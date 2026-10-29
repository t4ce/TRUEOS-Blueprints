//! Evidence that a Codex pack contains the native workload placement hooks.
//!
//! This receipt describes compiled capabilities. Carrier placement remains a
//! runtime setting; producing a Blueprint never changes the user's profile.

use crate::codex_artifact::{self, Verification};
use crate::{CargoMetadata, cargo_output::CargoBuildArtifacts};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const RECEIPT_VERSION: u32 = 1;
const SOURCE_FILES: &[(&str, &[&str])] = &[
    ("codex-cli", &["src/main.rs", "src/lib.rs"]),
    (
        "codex-arg0",
        &[
            "src/lib.rs",
            "src/trueos_threads.rs",
            "src/startup_timing.rs",
        ],
    ),
    (
        "tokio",
        &[
            "src/runtime/trueos.rs",
            "src/runtime/blocking/pool.rs",
            "src/runtime/blocking/task.rs",
            "src/runtime/scheduler/multi_thread/worker.rs",
        ],
    ),
];

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
struct SourcePackage {
    version: String,
    manifest: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(crate) struct Sources {
    packages: BTreeMap<String, SourcePackage>,
    files_sha256: BTreeMap<PathBuf, String>,
}

pub(crate) struct CliSources(BTreeMap<PathBuf, String>);

#[derive(Debug, Deserialize, Serialize)]
struct Receipt {
    version: u32,
    blueprint_sha256: String,
    artifact: Verification,
    sources: Sources,
}

pub(crate) fn receipt_path(blueprint: &Path) -> PathBuf {
    blueprint.with_extension("workload.json")
}

fn expected_manifests(app: &Path, sdk: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let (_, tokio_vendor) = crate::BLUEPRINT_VENDOR_PATCHES
        .iter()
        .find(|(name, _)| *name == "tokio")
        .ok_or("Codex workload guard requires the shared Tokio source patch")?;
    [
        ("codex-cli", app.join("Cargo.toml")),
        ("codex-arg0", app.join("../arg0/Cargo.toml")),
        (
            "tokio",
            crate::blueprint_vendor_path(sdk, tokio_vendor).join("Cargo.toml"),
        ),
    ]
    .into_iter()
    .map(|(name, path)| {
        fs::canonicalize(&path)
            .map(|path| (name.to_owned(), path))
            .map_err(|error| format!("Codex workload source {}: {error}", path.display()))
    })
    .collect()
}

/// Capture original CLI files before staging copies and rewrites them. A later
/// snapshot alone could attribute an edit after the copy to older staged code.
pub(crate) fn snapshot_cli(app: &Path) -> Result<CliSources, String> {
    let directory = fs::canonicalize(app).map_err(crate::io_string)?;
    let (_, files) = SOURCE_FILES
        .iter()
        .find(|(name, _)| *name == "codex-cli")
        .ok_or("Codex workload source inventory omitted the CLI")?;
    let files = files
        .iter()
        .map(|relative| {
            let path = directory.join(relative);
            codex_artifact::sha256(&path).map(|digest| (path, digest))
        })
        .collect::<Result<_, _>>()?;
    Ok(CliSources(files))
}

/// Use Cargo's selected graph, rather than guessing from a target directory.
pub(crate) fn from_metadata(
    app: &Path,
    sdk: &Path,
    staged_manifest: &Path,
    metadata: &CargoMetadata,
    cli_before_staging: &CliSources,
) -> Result<Sources, String> {
    let expected = expected_manifests(app, sdk)?;
    let staged_manifest = fs::canonicalize(staged_manifest).map_err(crate::io_string)?;
    let root = metadata
        .packages
        .iter()
        .find(|package| {
            fs::canonicalize(&package.manifest_path).ok().as_ref() == Some(&staged_manifest)
        })
        .ok_or("Codex workload guard: Cargo metadata omitted the requested CLI")?;
    if root.name != "codex-cli" {
        return Err("Codex workload guard: requested package is not codex-cli".into());
    }
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or("Codex workload guard: missing Cargo resolve graph")?;
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root.id.clone()];
    while let Some(id) = pending.pop() {
        if reachable.insert(id.clone())
            && let Some(node) = resolve.nodes.iter().find(|node| node.id == id)
        {
            pending.extend(node.deps.iter().map(|dep| dep.pkg.clone()));
        }
    }
    let mut packages = BTreeMap::new();
    for (name, expected_manifest) in &expected {
        let selected = metadata
            .packages
            .iter()
            .filter(|package| package.name == *name && reachable.contains(&package.id))
            .collect::<Vec<_>>();
        let [package] = selected.as_slice() else {
            return Err(format!(
                "Codex workload guard: expected one selected {name} source, found {}",
                selected.len()
            ));
        };
        let actual = fs::canonicalize(&package.manifest_path).map_err(crate::io_string)?;
        // The CLI itself is copied into the private staging tree; its sibling
        // crates and Tokio must still resolve to their authored source trees.
        if package.source.is_some() || (name != "codex-cli" && actual != *expected_manifest) {
            return Err(format!(
                "Codex workload guard: {name} resolved to {}, expected {}",
                actual.display(),
                expected_manifest.display()
            ));
        }
        packages.insert(
            name.clone(),
            SourcePackage {
                version: package.version.clone(),
                manifest: expected_manifest.clone(),
            },
        );
    }
    let mut sources = Sources {
        packages,
        files_sha256: BTreeMap::new(),
    };
    sources.files_sha256 = sources.snapshot()?;
    if cli_before_staging
        .0
        .iter()
        .any(|(path, digest)| sources.files_sha256.get(path) != Some(digest))
    {
        return Err(
            "Codex CLI workload sources changed during staging; repack to refresh the receipt"
                .into(),
        );
    }
    Ok(sources)
}

impl Sources {
    fn snapshot(&self) -> Result<BTreeMap<PathBuf, String>, String> {
        let mut files = BTreeMap::new();
        for (name, relative_paths) in SOURCE_FILES {
            let manifest = &self
                .packages
                .get(*name)
                .ok_or_else(|| format!("Codex workload receipt omitted {name}"))?
                .manifest;
            let directory = manifest
                .parent()
                .ok_or("invalid workload source manifest")?;
            for relative in *relative_paths {
                let path = directory.join(relative);
                files.insert(path.clone(), codex_artifact::sha256(&path)?);
            }
        }
        Ok(files)
    }

    fn validate_current(&self, app: &Path, sdk: &Path) -> Result<(), String> {
        let expected = expected_manifests(app, sdk)?;
        if self.packages.len() != expected.len()
            || expected.iter().any(|(name, manifest)| {
                self.packages
                    .get(name)
                    .is_none_or(|package| package.manifest != *manifest)
            })
        {
            return Err("Codex workload receipt describes different source trees".into());
        }
        if self.snapshot()? != self.files_sha256 {
            return Err("Codex workload sources changed; repack to refresh the receipt".into());
        }
        Ok(())
    }

    /// Check actual compiler artifacts too: feature-selected builds may differ
    /// from the default-feature metadata graph used during staging.
    pub(crate) fn verify_compiled(&self, artifacts: &CargoBuildArtifacts) -> Result<(), String> {
        for name in ["codex-arg0", "tokio"] {
            let expected = &self.packages[name].manifest;
            let mut seen = false;
            for manifest in artifacts.package_manifests.values() {
                if crate::package_name(manifest)? != name {
                    continue;
                }
                let actual = fs::canonicalize(manifest).map_err(crate::io_string)?;
                if actual != *expected {
                    return Err(format!(
                        "Codex workload guard: compiled {name} from {}, expected {}",
                        actual.display(),
                        expected.display()
                    ));
                }
                seen = true;
            }
            if !seen {
                return Err(format!("Codex workload guard: no compiled {name} artifact"));
            }
        }
        Ok(())
    }
}

pub(crate) fn write_receipt(
    blueprint: &Path,
    app: &Path,
    sdk: &Path,
    sources: Sources,
    artifact: Verification,
) -> Result<PathBuf, String> {
    sources.validate_current(app, sdk)?;
    artifact.validate()?;
    let receipt = Receipt {
        version: RECEIPT_VERSION,
        blueprint_sha256: codex_artifact::sha256(blueprint)?,
        artifact,
        sources,
    };
    let path = receipt_path(blueprint);
    let mut bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    fs::write(&path, bytes).map_err(crate::io_string)?;
    print_verified(blueprint, &receipt);
    Ok(path)
}

/// The complete build cache validates all dependency inputs. This additional
/// binding prevents its fast path from accepting an absent or unrelated receipt.
pub(crate) fn verify_cached(blueprint: &Path, app: &Path, sdk: &Path) -> Result<(), String> {
    let path = receipt_path(blueprint);
    let receipt: Receipt = serde_json::from_slice(&fs::read(&path).map_err(crate::io_string)?)
        .map_err(|error| format!("invalid Codex workload receipt {}: {error}", path.display()))?;
    if receipt.version != RECEIPT_VERSION {
        return Err("unsupported Codex workload receipt version".into());
    }
    receipt.artifact.validate()?;
    receipt.sources.validate_current(app, sdk)?;
    codex_artifact::verify_blueprint_digest(blueprint, &receipt.blueprint_sha256)?;
    print_verified(blueprint, &receipt);
    Ok(())
}

fn print_verified(blueprint: &Path, receipt: &Receipt) {
    println!(
        "trueos-blueprint: Codex workload verified: placement, child roles, execution activity; blueprint_sha256={}",
        receipt.blueprint_sha256
    );
    println!(
        "trueos-blueprint: Codex workload receipt: {}",
        receipt_path(blueprint).display()
    );
    println!(
        "trueos-blueprint: Codex runtime policy: single by default; isolated/spread selected at runtime by CODEX_TRUEOS_THREAD_PLACEMENT or $CODEX_HOME/trueos-threads.conf"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        app: PathBuf,
        sdk: PathBuf,
        staged: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "codex-workload-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let fixture = Self {
                app: root.join("codex/codex-rs/cli"),
                sdk: root.join("TRUEOS-Blueprints"),
                staged: root.join("stage/Cargo.toml"),
                root,
            };
            let directories = [
                ("codex-cli", fixture.app.clone()),
                ("codex-arg0", fixture.app.join("../arg0")),
                (
                    "tokio",
                    crate::blueprint_vendor_path(
                        &fixture.sdk,
                        crate::BLUEPRINT_VENDOR_PATCHES
                            .iter()
                            .find(|(name, _)| *name == "tokio")
                            .unwrap()
                            .1,
                    ),
                ),
            ];
            for (name, directory) in directories {
                fs::create_dir_all(&directory).unwrap();
                fs::write(
                    directory.join("Cargo.toml"),
                    format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"),
                )
                .unwrap();
                for relative in SOURCE_FILES
                    .iter()
                    .find(|(package, _)| *package == name)
                    .unwrap()
                    .1
                {
                    let path = directory.join(relative);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, format!("// source {name}/{relative}\n")).unwrap();
                }
            }
            fs::create_dir_all(fixture.staged.parent().unwrap()).unwrap();
            fs::copy(fixture.app.join("Cargo.toml"), &fixture.staged).unwrap();
            fixture
        }

        fn metadata(&self) -> CargoMetadata {
            let expected = expected_manifests(&self.app, &self.sdk).unwrap();
            serde_json::from_value(serde_json::json!({
                "packages": [
                    {"id":"cli", "name":"codex-cli", "version":"1.0.0", "manifest_path":self.staged, "dependencies":[]},
                    {"id":"arg0", "name":"codex-arg0", "version":"1.0.0", "manifest_path":expected["codex-arg0"], "dependencies":[]},
                    {"id":"tokio", "name":"tokio", "version":"1.0.0", "manifest_path":expected["tokio"], "dependencies":[]}
                ],
                "resolve":{"root":"cli", "nodes":[
                    {"id":"cli", "deps":[{"name":"arg0", "pkg":"arg0"}, {"name":"tokio", "pkg":"tokio"}]},
                    {"id":"arg0", "deps":[]}, {"id":"tokio", "deps":[]}
                ]}
            })).unwrap()
        }

        fn sources(&self) -> Sources {
            from_metadata(
                &self.app,
                &self.sdk,
                &self.staged,
                &self.metadata(),
                &snapshot_cli(&self.app).unwrap(),
            )
            .unwrap()
        }

        fn blueprint(&self) -> PathBuf {
            let path = self.root.join("codex.bp");
            fs::write(&path, b"fixture blueprint bytes").unwrap();
            path
        }

        fn verification(&self) -> Verification {
            Verification {
                elf_sha256: "a".repeat(64),
                required_imports: [
                    "trueos_cabi_thread_configure_placement_v1",
                    "trueos_cabi_thread_swap_activity_v1",
                    "trueos_cabi_thread_swap_child_role_v1",
                ]
                .map(String::from)
                .to_vec(),
            }
        }

        fn write_receipt(&self, blueprint: &Path) -> PathBuf {
            write_receipt(
                blueprint,
                &self.app,
                &self.sdk,
                self.sources(),
                self.verification(),
            )
            .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn records_original_cli_live_arg0_and_pinned_tokio_sources() {
        let fixture = Fixture::new();
        let sources = fixture.sources();
        assert_eq!(
            sources.packages["codex-cli"].manifest,
            fs::canonicalize(fixture.app.join("Cargo.toml")).unwrap()
        );
        assert_eq!(sources.files_sha256.len(), 9);
        assert!(
            sources
                .files_sha256
                .keys()
                .all(|path| !path.starts_with(fixture.staged.parent().unwrap()))
        );
        sources
            .validate_current(&fixture.app, &fixture.sdk)
            .unwrap();
    }

    #[test]
    fn rejects_registry_tokio_or_another_source_tree() {
        let fixture = Fixture::new();
        let mut metadata = fixture.metadata();
        metadata.packages[2].source =
            Some("registry+https://github.com/rust-lang/crates.io-index".into());
        assert!(
            from_metadata(
                &fixture.app,
                &fixture.sdk,
                &fixture.staged,
                &metadata,
                &snapshot_cli(&fixture.app).unwrap()
            )
            .unwrap_err()
            .contains("tokio resolved")
        );
        metadata.packages[2].source = None;
        metadata.packages[2].manifest_path = fixture.staged.clone();
        assert!(
            from_metadata(
                &fixture.app,
                &fixture.sdk,
                &fixture.staged,
                &metadata,
                &snapshot_cli(&fixture.app).unwrap()
            )
            .unwrap_err()
            .contains("tokio resolved")
        );
    }

    #[test]
    fn a_cli_edit_after_staging_copy_cannot_describe_the_older_staged_code() {
        let fixture = Fixture::new();
        let before_staging = snapshot_cli(&fixture.app).unwrap();
        let staged_source = fixture.staged.parent().unwrap().join("src/main.rs");
        fs::create_dir_all(staged_source.parent().unwrap()).unwrap();
        fs::copy(fixture.app.join("src/main.rs"), &staged_source).unwrap();
        fs::write(
            fixture.app.join("src/main.rs"),
            "// edited after staging copied the CLI\n",
        )
        .unwrap();
        let error = from_metadata(
            &fixture.app,
            &fixture.sdk,
            &fixture.staged,
            &fixture.metadata(),
            &before_staging,
        )
        .unwrap_err();
        assert!(error.contains("changed during staging"));
    }

    #[test]
    fn rejects_a_compiled_source_that_differs_from_metadata() {
        let fixture = Fixture::new();
        let sources = fixture.sources();
        let mut artifacts = CargoBuildArtifacts::default();
        for name in ["codex-arg0", "tokio"] {
            artifacts
                .package_manifests
                .insert(name.into(), sources.packages[name].manifest.clone());
        }
        sources.verify_compiled(&artifacts).unwrap();
        let wrong = fixture.root.join("other-tokio/Cargo.toml");
        fs::create_dir_all(wrong.parent().unwrap()).unwrap();
        fs::write(&wrong, "[package]\nname = \"tokio\"\n").unwrap();
        artifacts.package_manifests.insert("tokio".into(), wrong);
        assert!(
            sources
                .verify_compiled(&artifacts)
                .unwrap_err()
                .contains("compiled tokio")
        );
    }

    #[test]
    fn receipt_accepts_only_matching_blueprint_bytes() {
        let fixture = Fixture::new();
        let blueprint = fixture.blueprint();
        fixture.write_receipt(&blueprint);
        verify_cached(&blueprint, &fixture.app, &fixture.sdk).unwrap();
        fs::write(&blueprint, b"different blueprint bytes").unwrap();
        assert!(verify_cached(&blueprint, &fixture.app, &fixture.sdk).is_err());
    }

    #[test]
    fn missing_receipt_and_changed_source_prevent_cache_reuse() {
        let fixture = Fixture::new();
        let blueprint = fixture.blueprint();
        assert!(verify_cached(&blueprint, &fixture.app, &fixture.sdk).is_err());
        let receipt = fixture.write_receipt(&blueprint);
        let before = fixture.sources();
        fs::write(
            fixture.app.join("../arg0/src/trueos_threads.rs"),
            "// updated policy\n",
        )
        .unwrap();
        assert!(
            verify_cached(&blueprint, &fixture.app, &fixture.sdk)
                .unwrap_err()
                .contains("sources changed")
        );
        assert!(
            write_receipt(
                &blueprint,
                &fixture.app,
                &fixture.sdk,
                before,
                fixture.verification()
            )
            .is_err()
        );
        fs::remove_file(receipt).unwrap();
        assert!(verify_cached(&blueprint, &fixture.app, &fixture.sdk).is_err());
    }

    #[test]
    fn incomplete_hook_receipt_cannot_enter_cache_fast_path() {
        let fixture = Fixture::new();
        let blueprint = fixture.blueprint();
        let path = fixture.write_receipt(&blueprint);
        let mut receipt: Receipt = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        receipt.artifact.required_imports.pop();
        fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(
            verify_cached(&blueprint, &fixture.app, &fixture.sdk)
                .unwrap_err()
                .contains("import set")
        );
    }
}
