//! Unit tests for the Blueprint packer.

use super::*;

#[cfg(test)]
mod external_path_overlay_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    fn test_dir(name: &str) -> PathBuf {
        let serial = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "trueos-blueprint-{name}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated test directory");
        path
    }

    #[test]
    fn app_path_patch_becomes_an_audited_source_overlay() {
        let root = test_dir("path-patch");
        let app = root.join("app");
        let engine = root.join("engine-wgpu");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&engine).unwrap();
        fs::write(
            engine.join("Cargo.toml"),
            "[package]\nname = \"wgpu\"\nversion = \"30.0.0\"\n",
        )
        .unwrap();
        let manifest = app.join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[patch.crates-io]\nwgpu = { path = \"../engine-wgpu\" }\n",
        )
        .unwrap();

        let mut patches = Vec::new();
        add_manifest_source_patches(&manifest, &mut patches).unwrap();

        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].key, "wgpu");
        assert_eq!(patches[0].name, "wgpu");
        assert_eq!(patches[0].path, fs::canonicalize(&engine).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_patch_requires_an_immutable_revision() {
        let rev = "3786da941f0042c923fb5981a0edda86b8d38dba";
        assert_eq!(
            pinned_git_patch_revision("stylo", &format!("{{ rev = \"{rev}\" }}")).unwrap(),
            rev
        );
        for value in [
            "{ branch = \"main\" }".to_owned(),
            "{ rev = \"3786da9\" }".to_owned(),
            format!("{{ rev = \"{rev}\", tag = \"v0.20.0\" }}"),
        ] {
            assert!(pinned_git_patch_revision("stylo", &value).is_err());
        }
    }

    #[test]
    fn git_patch_selects_only_the_locked_fork_commit() {
        let git = "https://github.com/t4ce/stylo";
        let rev = "3786da941f0042c923fb5981a0edda86b8d38dba";
        let metadata: CargoMetadata = serde_json::from_value(serde_json::json!({
            "packages": [
                { "id": "registry-stylo", "name": "stylo", "version": "0.20.0",
                  "source": "registry+https://github.com/rust-lang/crates.io-index",
                  "manifest_path": "/registry/stylo/Cargo.toml", "dependencies": [] },
                { "id": "fork-stylo", "name": "stylo", "version": "0.20.0",
                  "source": format!("git+{git}?rev={rev}#{rev}"),
                  "manifest_path": "/checkout/style/Cargo.toml", "dependencies": [] }
            ],
            "resolve": null
        }))
        .unwrap();
        assert_eq!(
            pinned_git_patch_path(&metadata, "stylo", git, rev).unwrap(),
            PathBuf::from("/checkout/style")
        );
        assert!(
            pinned_git_patch_path(&metadata, "stylo", "https://github.com/servo/stylo", rev)
                .is_err()
        );
        assert!(
            pinned_git_patch_path(
                &metadata,
                "stylo",
                git,
                "0000000000000000000000000000000000000000"
            )
            .is_err()
        );
    }

    #[test]
    fn overlay_version_audit_reads_workspace_inheritance() {
        let root = test_dir("workspace-version");
        let member = root.join("style");
        fs::create_dir_all(&member).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"style\"]\n[workspace.package]\nversion = \"0.20.0\"\n",
        )
        .unwrap();
        let manifest = member.join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"stylo\"\nversion.workspace = true\n",
        )
        .unwrap();
        assert_eq!(
            package_version(&manifest).unwrap().as_deref(),
            Some("0.20.0")
        );
        fs::remove_file(root.join("Cargo.toml")).unwrap();
        assert!(package_version(&manifest).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_relative_dependency_keeps_its_original_package_identity() {
        let root = test_dir("staged-path");
        let app = root.join("app");
        let engine = root.join("engine");
        let staged = root.join("staged/Cargo.toml");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&engine).unwrap();
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(
            engine.join("Cargo.toml"),
            "[package]\nname = \"engine\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let source = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nengine = { path = \"../engine\" }\n";
        let original = app.join("Cargo.toml");
        fs::write(&original, source).unwrap();
        fs::write(&staged, source).unwrap();

        canonicalize_staged_manifest_paths_from_original(&original, &staged).unwrap();

        let rewritten = fs::read_to_string(&staged).unwrap();
        let canonical = fs::canonicalize(engine).unwrap();
        assert!(rewritten.contains(&format!(
            "engine = {{ path = \"{}\" }}",
            canonical.display()
        )));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_workspace_only_keeps_the_selected_root_package() {
        let root = test_dir("staged-workspace-members");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\n    \".\",\n    \"crates/renderer\",\n]\ndefault-members = [\"crates/renderer\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nserde = \"1\"\n",
        )
        .unwrap();

        isolate_staged_workspace_members(&manifest).unwrap();

        assert_eq!(
            fs::read_to_string(&manifest).unwrap(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\".\"]\ndefault-members = [\".\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nserde = \"1\"\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod source_rewrite_tests {
    use super::*;

    #[test]
    fn rewrites_supported_collection_imports_without_touching_hash_map_internals() {
        let source = "\
use std::collections::HashMap;
use std::collections::{HashMap, HashSet};
use std::collections::{HashMap, hash_map::{Iter, Keys}};
type Ordered = std::collections::BTreeMap<u8, u8>;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
let start = std::time::Instant::now();
";

        let rewritten = rewrite_trueos_collection_imports_in_source(source);

        assert!(rewritten.contains("use trueos::collections::HashMap;"));
        assert!(rewritten.contains("use trueos::collections::{HashMap, HashSet};"));
        assert!(rewritten.contains("type Ordered = trueos::collections::BTreeMap<u8, u8>;"));
        assert!(rewritten.contains("use std::collections::{HashMap, hash_map::{Iter, Keys}};"));
        assert!(rewritten.contains("use std::time::{Duration, SystemTime, UNIX_EPOCH};"));
        assert!(rewritten.contains("use trueos::clock::Instant;"));
        assert!(rewritten.contains("let start = trueos::clock::Instant::now();"));
    }
}

#[cfg(test)]
mod rustc_payload_tests {
    use super::*;

    fn dependency(name: &str, package_id: &str, kind: Option<&str>) -> MetadataNodeDep {
        MetadataNodeDep {
            name: name.to_owned(),
            pkg: package_id.to_owned(),
            dep_kinds: vec![MetadataDepKind {
                kind: kind.map(str::to_owned),
            }],
        }
    }

    fn artifact(
        package_id: &str,
        crate_name: &str,
        file_name: &str,
    ) -> cargo_output::CargoTargetMetadataArtifact {
        cargo_output::CargoTargetMetadataArtifact {
            package_id: package_id.to_owned(),
            crate_name: crate_name.to_owned(),
            source_path: PathBuf::from(format!("/source/{crate_name}/lib.rs")),
            path: PathBuf::from(format!("/target/deps/{file_name}")),
        }
    }

    #[test]
    fn payload_selection_walks_normal_target_closure_by_package_id() {
        let metadata = CargoMetadata {
            packages: Vec::new(),
            resolve: Some(MetadataResolve {
                root: Some("root".to_owned()),
                nodes: vec![
                    MetadataNode {
                        id: "root".to_owned(),
                        features: Vec::new(),
                        deps: vec![
                            dependency("payload-alias", "payload", None),
                            dependency("host-tool", "host", Some("build")),
                        ],
                    },
                    MetadataNode {
                        id: "payload".to_owned(),
                        features: Vec::new(),
                        deps: vec![
                            dependency("same_v1", "same-1", None),
                            dependency("same_v2", "same-2", None),
                            dependency("build-helper", "build-helper", Some("build")),
                        ],
                    },
                    MetadataNode {
                        id: "same-1".to_owned(),
                        features: Vec::new(),
                        deps: Vec::new(),
                    },
                    MetadataNode {
                        id: "same-2".to_owned(),
                        features: Vec::new(),
                        deps: Vec::new(),
                    },
                ],
            }),
        };
        let artifacts = vec![
            artifact("build-helper", "build_helper", "libbuild_helper-a.rmeta"),
            artifact("host", "host_tool", "libhost_tool-a.rmeta"),
            artifact("payload", "payload_lib", "libpayload_lib-a.rmeta"),
            artifact("same-1", "same", "libsame-1111111111111111.rmeta"),
            artifact("same-2", "same", "libsame-2222222222222222.rmeta"),
        ];

        let selected =
            select_rustc_payload(&metadata, &["payload-alias".to_owned()], &artifacts).unwrap();

        assert_eq!(selected.direct_externs.len(), 1);
        assert_eq!(selected.direct_externs[0].alias, "payload_alias");
        assert_eq!(selected.direct_externs[0].crate_name, "payload_lib");
        assert_eq!(
            selected
                .artifacts
                .iter()
                .map(|artifact| artifact.package_id.as_str())
                .collect::<Vec<_>>(),
            vec!["payload", "same-1", "same-2"]
        );
    }

    #[test]
    fn payload_selection_rejects_alias_actual_name_collision() {
        let metadata = CargoMetadata {
            packages: Vec::new(),
            resolve: Some(MetadataResolve {
                root: Some("root".to_owned()),
                nodes: vec![
                    MetadataNode {
                        id: "root".to_owned(),
                        features: Vec::new(),
                        deps: vec![dependency("same", "payload", None)],
                    },
                    MetadataNode {
                        id: "payload".to_owned(),
                        features: Vec::new(),
                        deps: vec![dependency("transitive", "transitive", None)],
                    },
                    MetadataNode {
                        id: "transitive".to_owned(),
                        features: Vec::new(),
                        deps: Vec::new(),
                    },
                ],
            }),
        };
        let artifacts = vec![
            artifact("payload", "payload_lib", "libpayload_lib-a.rmeta"),
            artifact("transitive", "same", "libsame-a.rmeta"),
        ];

        let error = select_rustc_payload(&metadata, &["same".to_owned()], &artifacts).unwrap_err();

        assert!(error.contains("collides"));
    }

    #[test]
    fn isolated_payload_dependency_keeps_only_resolved_api_features() {
        let metadata = CargoMetadata {
            packages: vec![
                MetadataPackage {
                    id: "root".to_owned(),
                    name: "compiler".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: None,
                    manifest_path: PathBuf::new(),
                    dependencies: vec![MetadataDependency {
                        name: "trueos".to_owned(),
                        rename: None,
                        req: "*".to_owned(),
                        path: Some(PathBuf::from("/sdk/api")),
                    }],
                    features: BTreeMap::new(),
                },
                MetadataPackage {
                    id: "trueos".to_owned(),
                    name: "trueos".to_owned(),
                    version: "0.1.0".to_owned(),
                    source: None,
                    manifest_path: PathBuf::new(),
                    dependencies: Vec::new(),
                    features: BTreeMap::from([
                        ("default-global-allocator".to_owned(), Vec::new()),
                        ("tokio-runtime".to_owned(), Vec::new()),
                    ]),
                },
            ],
            resolve: Some(MetadataResolve {
                root: Some("root".to_owned()),
                nodes: vec![
                    MetadataNode {
                        id: "root".to_owned(),
                        features: vec!["host-compiler".to_owned()],
                        deps: vec![dependency("trueos", "trueos", None)],
                    },
                    MetadataNode {
                        id: "trueos".to_owned(),
                        features: vec![
                            "default-global-allocator".to_owned(),
                            "dep:internal-detail".to_owned(),
                        ],
                        deps: Vec::new(),
                    },
                ],
            }),
        };

        let dependencies =
            rustc_payload_dependencies(&metadata, &["trueos".to_owned()]).unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].alias, "trueos");
        assert_eq!(dependencies[0].package_name, "trueos");
        assert_eq!(dependencies[0].version, "0.1.0");
        assert_eq!(dependencies[0].path.as_deref(), Some(Path::new("/sdk/api")));
        assert_eq!(
            dependencies[0].features,
            vec!["default-global-allocator"]
        );
    }
}

#[cfg(test)]
mod version_alignment_tests {
    use super::{MetadataDependency, declared_dependency_for_resolved_edge};

    fn dependency(name: &str, rename: Option<&str>, req: &str) -> MetadataDependency {
        MetadataDependency {
            name: name.to_string(),
            rename: rename.map(str::to_string),
            req: req.to_string(),
            path: None,
        }
    }

    #[test]
    fn resolved_dependency_name_selects_the_matching_compatibility_alias() {
        let dependencies = vec![
            dependency("mio", Some("mio-0_6"), "~0.6"),
            dependency("mio", Some("mio-0_7"), "~0.7"),
            dependency("mio", Some("mio-0_8"), "~0.8"),
            dependency("mio", Some("mio-1_0"), "1.0"),
        ];

        let selected = declared_dependency_for_resolved_edge(&dependencies, "mio_1_0")
            .expect("resolved mio compatibility edge");

        assert_eq!(selected.rename.as_deref(), Some("mio-1_0"));
        assert_eq!(selected.req, "1.0");
        assert!(declared_dependency_for_resolved_edge(&dependencies, "mio").is_none());
    }

    #[test]
    fn resolved_dependency_name_normalizes_unrenamed_hyphens() {
        let dependencies = vec![dependency("signal-hook", None, "~0.3")];

        let selected = declared_dependency_for_resolved_edge(&dependencies, "signal_hook")
            .expect("resolved signal-hook edge");

        assert_eq!(selected.name, "signal-hook");
    }
}

#[cfg(test)]
mod workspace_dependency_tests {
    use super::*;

    #[test]
    fn vendored_workspace_dependency_keeps_the_overlay_canonical_path() {
        let canonical = PathBuf::from("/sdk/vendor/hyper-1.9.0");
        let patches = [CratePatch::new("hyper", canonical.clone())];

        let resolved = workspace_dependency_vendor_path(
            &patches,
            Path::new("/staging/work/package"),
            "hyper",
            Path::new("/fallback/vendor/hyper-1.9.0"),
        );

        assert_eq!(resolved, canonical);
    }

    #[test]
    fn trueos_workspace_dependency_uses_the_canonical_sdk_path() {
        let resolved = materialized_workspace_dependency(
            Path::new("/sdk/apps/example"),
            Path::new("/sdk"),
            Path::new("/staging/work/example"),
            &[],
            "trueos",
        )
        .expect("materialize trueos workspace dependency");

        assert_eq!(resolved, "trueos = { path = \"/sdk/api\" }");
    }

    #[test]
    fn direct_trueos_dependency_rewrite_preserves_options() {
        let line =
            "trueos = { path = \"../../api\", features = [\"tokio-runtime\"], optional = true }";
        let rewritten = dependency_with_rewritten_path(line, "trueos", Path::new("/sdk/api"))
            .expect("rewrite direct trueos dependency");

        assert_eq!(
            rewritten,
            "trueos = { path = \"/sdk/api\", features = [\"tokio-runtime\"], optional = true }"
        );
    }

    #[test]
    fn direct_blueprint_dependency_rewrite_supports_other_crates() {
        let line = "trueos-math = { path = \"../../crates/trueos-math\" }";
        let (dependency, _) = inline_dependency_name_and_path(line)
            .expect("recognize direct Blueprint crate dependency");
        assert_eq!(
            dependency_with_rewritten_path(line, dependency, Path::new("/sdk/crates/trueos-math"),),
            Some("trueos-math = { path = \"/sdk/crates/trueos-math\" }".to_string())
        );
    }
}

#[cfg(test)]
mod terminal_platform_vendor_tests {
    use super::*;

    #[test]
    fn terminal_stack_is_owned_by_the_blueprint_platform_vendor() {
        for (package, vendor_dir) in [
            ("crossterm", "crossterm-0.29.0-trueos"),
            ("rustix", "rustix-1.1.4-trueos"),
            ("signal-hook-mio", "signal-hook-mio-0.2.5-trueos"),
        ] {
            assert!(BLUEPRINT_VENDOR_PATCHES.contains(&(package, vendor_dir)));
            assert!(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("vendor")
                    .join(vendor_dir)
                    .join("Cargo.toml")
                    .is_file()
            );
        }
    }

    #[test]
    fn terminal_platform_forks_use_persistent_mio_eventflow() {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
        let crossterm =
            fs::read_to_string(vendor.join("crossterm-0.29.0-trueos/src/event/source/unix/mio.rs"))
                .unwrap();
        let signal_hook =
            fs::read_to_string(vendor.join("signal-hook-mio-0.2.5-trueos/src/lib.rs")).unwrap();

        assert_eq!(crossterm, CROSSTERM_TRUEOS_MIO_SOURCE);
        assert!(crossterm.contains("trueos_set_nonblocking(tty_raw_fd)?;"));
        assert!(crossterm.contains("trueos_cabi_blueprint_terminal_surface_snapshot_v1"));
        assert!(crossterm.contains("self.poll.poll(&mut self.events, timeout.leftover())"));
        assert!(!crossterm.contains("SURFACE_POLL_SLICE"));
        assert!(!crossterm.contains("crossterm-resize-probe"));
        assert!(!crossterm.contains("trueos_mio_selector"));
        assert!(!crossterm.contains("mio::io"));
        assert!(signal_hook.contains("type Error = MioIoError;"));
        assert!(!signal_hook.contains("mio::io"));
    }
}
