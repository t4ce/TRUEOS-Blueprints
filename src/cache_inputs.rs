//! Inputs for reusing a previously packed Blueprint without running Cargo.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

pub(crate) fn environment() -> String {
    let mut variables = env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            !matches!(
                key.as_ref(),
                "PWD" | "OLDPWD" | "SHLVL" | "_" | "CARGO_MAKEFLAGS" | "MAKEFLAGS" | "MFLAGS"
            ) && !(key.starts_with("TRUEOS_BLUEPRINT_") && key.contains("PUBLISH"))
        })
        .collect::<Vec<_>>();
    variables.sort();
    let mut hash = Sha256::new();
    for (key, value) in variables {
        for bytes in [key.as_encoded_bytes(), value.as_encoded_bytes()] {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
    format!("{:x}", hash.finalize())
}

pub(crate) fn base(app: &Path, manifest: &Path, target: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = vec![
        app.to_path_buf(),
        manifest.to_path_buf(),
        target.to_path_buf(),
        env::current_exe().map_err(crate::io_string)?,
    ];
    for directory in [
        app.to_path_buf(),
        env::current_dir().map_err(crate::io_string)?,
    ] {
        add_ancestor_config(&directory, &mut roots);
    }
    if let Some(home) = env::var_os("HOME") {
        let cargo_home = env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".cargo"));
        roots.extend([cargo_home.join("config"), cargo_home.join("config.toml")]);
    }
    if let Some(sdk) = crate::blueprint_root(app) {
        roots.extend([
            sdk.join("api"),
            sdk.join("crates"),
            sdk.join("Cargo.toml"),
            sdk.join("Cargo.lock"),
            sdk.join("rust-toolchain.toml"),
        ]);
        for (_, directory) in crate::BLUEPRINT_VENDOR_PATCHES
            .iter()
            .chain(crate::TRACING_VENDOR_PATCHES)
        {
            roots.push(crate::blueprint_vendor_path(&sdk, directory).join("Cargo.toml"));
            for ancestor in app.ancestors() {
                roots.push(ancestor.join("vendor").join(directory).join("Cargo.toml"));
            }
        }
        let buildins = sdk.join("../TRUEOS-Buildins");
        roots.push(buildins.join("tools"));
        if let Some(kernel) = crate::abi_guard::locate_kernel_repo(&sdk)? {
            roots.extend([
                kernel.join("tools/rust-std"),
                kernel.join("tools/testpy"),
                kernel.join("crates/trueos-v/src/bp_abi.rs"),
                kernel.join("abi/portal-cabi-v2.sha256"),
            ]);
        }
        // This asset is read by cubesrv's build script outside its Cargo package.
        if crate::package_name(manifest)? == "cubesrv" {
            roots.push(
                env::var_os("CUBES_WORLD1_SOURCE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| sdk.join("../Cubes/Cube/lvl27/world_01_sky.cubes")),
            );
        }
    }
    roots.extend(
        crate::package_blueprint_build_env_paths(manifest)?
            .into_iter()
            .map(|(_, path)| path),
    );
    // Known native-library and authored-asset overrides. Other build-script
    // paths are learned from their actual rerun-if-changed output below.
    for key in [
        "GAMES_CHESS_ASSETS_DIR",
        "CUBES_WORLD1_SOURCE",
        "LIBCLANG_PATH",
        "SDKROOT",
        "PKG_CONFIG_SYSROOT_DIR",
        "OPENSSL_DIR",
        "OPENSSL_LIB_DIR",
        "OPENSSL_INCLUDE_DIR",
    ] {
        if let Some(path) = env::var_os(key) {
            roots.push(PathBuf::from(path));
        }
    }
    roots.push(PathBuf::from("/var/lib/dpkg/status"));
    let toolchain = crate::toolchain::rust_sysroot()?;
    roots.extend([
        toolchain.join("bin"),
        toolchain.join("lib/rustlib/src/rust/library"),
    ]);
    // Driver/shared-library updates matter even when `rustc -vV` is unchanged.
    for entry in fs::read_dir(toolchain.join("lib")).map_err(crate::io_string)? {
        let path = entry.map_err(crate::io_string)?.path();
        if path.is_file() {
            roots.push(path);
        }
    }
    let host = crate::toolchain::rustc_identity()?.host;
    roots.extend([
        toolchain.join("lib/rustlib").join(&host).join("bin"),
        toolchain.join("lib/rustlib").join(host).join("lib"),
    ]);
    if let Some(search_path) = env::var_os("PATH") {
        let paths = env::split_paths(&search_path).collect::<Vec<_>>();
        for name in [
            "cc",
            "c++",
            "clang",
            "ar",
            "ld",
            "ld.lld",
            "lld",
            "llvm-objcopy",
            "objcopy",
            "tar",
            "lz4",
            "python3",
        ] {
            if let Some(path) = paths
                .iter()
                .map(|dir| dir.join(name))
                .find(|path| path.is_file())
            {
                roots.push(path);
            }
        }
    }
    for key in [
        "CC",
        "CXX",
        "AR",
        "LD",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        if let Some(value) = env::var_os(key) {
            let path = PathBuf::from(value);
            if path.is_file() {
                roots.push(path);
            }
        }
    }
    Ok(roots.into_iter().map(|path| absolute(&path)).collect())
}

fn add_ancestor_config(directory: &Path, roots: &mut Vec<PathBuf>) {
    for parent in directory.ancestors() {
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config",
            ".cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
        ] {
            roots.push(parent.join(name));
        }
    }
}

#[derive(Clone)]
pub(crate) struct Dependencies {
    roots: Vec<PathBuf>,
    packages: BTreeMap<String, PathBuf>,
    mappings: Vec<(PathBuf, PathBuf)>,
    work: PathBuf,
}

impl Dependencies {
    pub(crate) fn from_metadata(
        metadata: &crate::CargoMetadata,
        staged_manifest: &Path,
        app: &Path,
        work: &Path,
    ) -> Result<Self, String> {
        let staged_app = work.join("source-overlay-app");
        let mut result = Self {
            roots: Vec::new(),
            packages: BTreeMap::new(),
            mappings: vec![(staged_app, app.to_path_buf())],
            work: work.to_path_buf(),
        };
        if let Some(sdk) = crate::blueprint_root(app) {
            result.mappings.insert(
                0,
                (
                    work.join("blueprint-crates/trueos-tetris"),
                    sdk.join("../uiout/crates/trueos-tetris"),
                ),
            );
        }
        let root = metadata
            .packages
            .iter()
            .find(|package| absolute(&package.manifest_path) == absolute(staged_manifest))
            .ok_or("cache input discovery: Cargo metadata omitted requested package")?;
        let mut reachable = BTreeSet::new();
        let mut pending = vec![root.id.clone()];
        let resolve = metadata
            .resolve
            .as_ref()
            .ok_or("cache input discovery: missing Cargo resolve graph")?;
        while let Some(id) = pending.pop() {
            if reachable.insert(id.clone()) {
                if let Some(node) = resolve.nodes.iter().find(|node| node.id == id) {
                    pending.extend(node.deps.iter().map(|dep| dep.pkg.clone()));
                }
            }
        }
        for package in metadata
            .packages
            .iter()
            .filter(|package| reachable.contains(&package.id))
        {
            let staged = package
                .manifest_path
                .parent()
                .ok_or("cache input discovery: invalid manifest path")?;
            let original = if staged.starts_with(work.join("source-overlay-crates")) {
                let version = crate::package_version(&package.manifest_path)?
                    .ok_or("cache input discovery: missing overlay version")?;
                crate::find_cargo_registry_crate(&format!("{}-{version}", package.name))
                    .ok_or_else(|| {
                        format!(
                            "cache input discovery: missing original source for {} {version}",
                            package.name
                        )
                    })?
            } else {
                result.original(staged)
            };
            result
                .mappings
                .push((absolute(staged), absolute(&original)));
            result.packages.insert(package.id.clone(), absolute(staged));
            result.roots.push(absolute(&original));
            add_ancestor_config(&original, &mut result.roots);
        }
        if result.roots.iter().any(|path| path.starts_with(work)) {
            return Err("cache input discovery: unmapped generated dependency".into());
        }
        // Match the longest staged prefix before the app tree's broader prefix.
        result
            .mappings
            .sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
        Ok(result)
    }

    pub(crate) fn roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }

    fn original(&self, path: &Path) -> PathBuf {
        let path = absolute(path);
        for (staged, original) in &self.mappings {
            if let Ok(relative) = path.strip_prefix(staged) {
                return absolute(&original.join(relative));
            }
        }
        path
    }

    fn generated(&self, path: &Path, cargo_target: &Path) -> bool {
        let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        resolved.starts_with(cargo_target) || resolved.starts_with(&self.work)
    }

    pub(crate) fn extra_inputs(
        &self,
        artifacts: &crate::cargo_output::CargoBuildArtifacts,
        cargo_target: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        // The actual compiler stream includes optional-feature and build-std
        // packages that a default-feature metadata graph may not contain.
        let mut effective = self.clone();
        for manifest in artifacts.package_manifests.values() {
            let directory = manifest
                .parent()
                .ok_or("cache input discovery: invalid artifact manifest")?;
            if directory.starts_with(self.work.join("source-overlay-crates")) {
                let name = crate::package_name(manifest)?;
                let version = crate::package_version(manifest)?
                    .ok_or("cache input discovery: missing artifact version")?;
                let original = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
                    .ok_or_else(|| format!("cache input discovery: missing original artifact source {name}-{version}"))?;
                effective
                    .mappings
                    .insert(0, (directory.to_path_buf(), original));
            } else if effective.original(directory).starts_with(&self.work)
                && !directory.starts_with(self.work.join("rustc-payload-min"))
            {
                return Err("cache input discovery: unmapped generated artifact dependency".into());
            }
        }
        effective
            .mappings
            .sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
        effective.collect_extra_inputs(artifacts, cargo_target)
    }

    fn collect_extra_inputs(
        &self,
        artifacts: &crate::cargo_output::CargoBuildArtifacts,
        cargo_target: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        let mut inputs = artifacts
            .input_files
            .iter()
            .map(|path| self.original(path))
            // OUT_DIR sources are derived artifacts; their source trees and script inputs are tracked.
            .filter(|path| !self.generated(path, cargo_target))
            .collect::<Vec<_>>();
        inputs.extend(
            artifacts
                .package_manifests
                .values()
                .filter_map(|path| path.parent())
                .map(|path| self.original(path))
                .filter(|path| !self.generated(path, cargo_target)),
        );
        for script in &artifacts.build_script_outputs {
            let directory = self
                .packages
                .get(&script.package_id)
                .cloned()
                .or_else(|| {
                    artifacts
                        .package_manifests
                        .get(&script.package_id)
                        .and_then(|path| path.parent())
                        .map(Path::to_path_buf)
                })
                .ok_or_else(|| {
                    format!(
                        "cache input discovery: unknown build script {}",
                        script.package_id
                    )
                })?;
            let output = fs::read_to_string(&script.path).map_err(crate::io_string)?;
            for line in output.lines() {
                if let Some(path) = line
                    .strip_prefix("cargo:rerun-if-changed=")
                    .or_else(|| line.strip_prefix("cargo::rerun-if-changed="))
                {
                    let path = self.original(&directory.join(path));
                    if !self.generated(&path, cargo_target) {
                        inputs.push(path);
                    }
                }
            }
        }
        Ok(inputs)
    }
}

fn absolute(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_default().join(path)
    };
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "blueprint-cache-inputs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("app")).unwrap();
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn metadata_tracks_selected_dependency_closure_and_original_app() {
        let fixture = Fixture::new();
        let app = fixture.0.join("app");
        let work = app.join("target/work");
        let staged = work.join("source-overlay-app/Cargo.toml");
        let dependency = fixture.0.join("dependency");
        let unrelated = fixture.0.join("unrelated");
        let package = |id: &str, manifest: PathBuf| {
            serde_json::json!({
                "id": id, "name": id, "version": "0.1.0", "manifest_path": manifest, "dependencies": []
            })
        };
        let metadata = serde_json::from_value(serde_json::json!({
            "packages": [package("app", staged.clone()), package("dependency", dependency.join("Cargo.toml")), package("unrelated", unrelated.join("Cargo.toml"))],
            "resolve": {"root": "app", "nodes": [
                {"id": "app", "deps": [{"name": "dependency", "pkg": "dependency"}]},
                {"id": "dependency", "deps": []}, {"id": "unrelated", "deps": []}
            ]}
        })).unwrap();
        let inputs = Dependencies::from_metadata(&metadata, &staged, &app, &work).unwrap();
        assert!(inputs.roots().contains(&app));
        assert!(inputs.roots().contains(&dependency));
        assert!(!inputs.roots().contains(&unrelated));
        assert!(!inputs.roots().iter().any(|path| path.starts_with(&work)));
    }

    #[test]
    fn build_script_external_paths_and_feature_only_packages_are_tracked() {
        let fixture = Fixture::new();
        let app = fixture.0.join("app");
        let work = app.join("target/work");
        let target = fixture.0.join("cargo-target");
        fs::create_dir_all(&target).unwrap();
        let feature_package = fixture.0.join("optional-dependency");
        fs::create_dir_all(&feature_package).unwrap();
        let output = target.join("script-output");
        fs::write(
            &output,
            "cargo::rerun-if-changed=../external asset\ncargo:rerun-if-changed=data\n",
        )
        .unwrap();
        let mut artifacts = crate::cargo_output::CargoBuildArtifacts::default();
        artifacts
            .package_manifests
            .insert("optional".into(), feature_package.join("Cargo.toml"));
        artifacts
            .build_script_outputs
            .push(crate::cargo_output::CargoBuildScriptOutput {
                package_id: "optional".into(),
                path: output,
            });
        artifacts.input_files.push(target.join("generated.rs"));
        let inputs = Dependencies {
            roots: vec![],
            packages: BTreeMap::new(),
            mappings: vec![],
            work,
        };
        let extra = inputs.extra_inputs(&artifacts, &target).unwrap();
        assert!(extra.contains(&feature_package));
        assert!(extra.contains(&feature_package.join("../external asset")));
        assert!(extra.contains(&feature_package.join("data")));
        assert!(!extra.iter().any(|path| path.starts_with(&target)));
    }

    #[test]
    fn unknown_generated_dependency_disables_reuse() {
        let fixture = Fixture::new();
        let work = fixture.0.join("app/target/work");
        let inputs = Dependencies {
            roots: vec![],
            packages: BTreeMap::new(),
            mappings: vec![],
            work: work.clone(),
        };
        let mut artifacts = crate::cargo_output::CargoBuildArtifacts::default();
        artifacts
            .package_manifests
            .insert("unknown".into(), work.join("unmapped/Cargo.toml"));
        assert!(
            inputs
                .extra_inputs(&artifacts, &fixture.0.join("cargo-target"))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_components_after_symlinks_keep_filesystem_meaning() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("external/nested")).unwrap();
        fs::write(fixture.0.join("external/asset"), "external").unwrap();
        std::os::unix::fs::symlink(
            fixture.0.join("external/nested"),
            fixture.0.join("app/link"),
        )
        .unwrap();
        let path = absolute(&fixture.0.join("app/link/../asset"));
        assert_eq!(fs::read_to_string(path).unwrap(), "external");
    }

    #[test]
    fn stage_paths_map_back_to_original_sources() {
        let dependencies = Dependencies {
            roots: vec![],
            work: PathBuf::from("/app/target/work"),
            packages: BTreeMap::new(),
            mappings: vec![
                (
                    PathBuf::from("/app/target/work/source-overlay-crates/timer"),
                    PathBuf::from("/registry/timer"),
                ),
                (
                    PathBuf::from("/app/target/work/source-overlay-app"),
                    PathBuf::from("/app"),
                ),
            ],
        };
        assert_eq!(
            dependencies.original(Path::new(
                "/app/target/work/source-overlay-app/src/../assets/file"
            )),
            PathBuf::from("/app/src/../assets/file")
        );
        assert_eq!(
            dependencies.original(Path::new(
                "/app/target/work/source-overlay-crates/timer/src/lib.rs"
            )),
            PathBuf::from("/registry/timer/src/lib.rs")
        );
        assert_eq!(
            dependencies.original(Path::new("/external/source")),
            PathBuf::from("/external/source")
        );
    }
}
