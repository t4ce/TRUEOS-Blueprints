use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io_string;

/// Artifacts whose identity is authenticated by Cargo's current JSON stream.
///
/// Cargo reports every artifact it produced or reused. Native inputs are
/// selected separately from the root crate's `.rlink`; this inventory supplies
/// their authenticated package and source identities, never the link closure.
#[derive(Default)]
pub(crate) struct CargoBuildArtifacts {
    /// Metadata artifacts for the standard-library closure emitted by this
    /// Cargo invocation.
    ///
    /// This is intentionally populated from Cargo's JSON stream rather than
    /// by inspecting `deps_dir`: that directory is a persistent cache and can
    /// contain incompatible artifacts from earlier builds.
    pub(crate) sysroot_metadata: Vec<CargoTargetMetadataArtifact>,
    /// Every target metadata artifact emitted or reused by this Cargo
    /// invocation in the exact target dependency directory.
    ///
    /// Rustc Blueprint payload closures are selected from this authenticated
    /// inventory by Cargo package ID. Nothing is discovered by scraping the
    /// persistent target cache.
    pub(crate) target_metadata: Vec<CargoTargetMetadataArtifact>,
    /// Target archives emitted or reused by this invocation. The `.rlink`
    /// intersects this inventory when assigning native ownership folds.
    pub(crate) target_archives: Vec<CargoNativeArchiveArtifact>,
    /// Source and include files recorded in dep-info for the artifacts in
    /// this invocation, including host tools and procedural macros.
    pub(crate) input_files: Vec<PathBuf>,
    /// Build script stdout belonging to scripts executed or reused by this
    /// invocation. The package identity resolves relative rerun-if-changed
    /// paths against that package's manifest directory.
    pub(crate) build_script_outputs: Vec<CargoBuildScriptOutput>,
    /// Exact manifests for all packages whose targets Cargo built or reused,
    /// including build-std packages absent from the app's metadata graph.
    pub(crate) package_manifests: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CargoBuildScriptOutput {
    pub(crate) package_id: String,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct CargoNativeArchiveArtifact {
    pub(crate) package_id: String,
    pub(crate) crate_name: String,
    pub(crate) source_path: PathBuf,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CargoTargetMetadataArtifact {
    pub(crate) package_id: String,
    pub(crate) crate_name: String,
    pub(crate) source_path: PathBuf,
    pub(crate) path: PathBuf,
}

#[derive(Deserialize)]
struct CargoJsonMessage {
    reason: String,
    #[serde(default)]
    package_id: Option<String>,
    #[serde(default)]
    target: Option<CargoJsonTarget>,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    message: Option<CargoJsonDiagnostic>,
    #[serde(default)]
    out_dir: Option<String>,
    #[serde(default)]
    manifest_path: Option<String>,
}

#[derive(Deserialize)]
struct CargoJsonTarget {
    name: String,
    src_path: String,
}

#[derive(Deserialize)]
struct CargoJsonDiagnostic {
    rendered: Option<String>,
}

// These are the metadata crates shipped in the pinned toolchain's target
// sysroot, including the platform-dependent backtrace closure. Cargo target
// names use underscores even when their package names use hyphens.
const SYSROOT_CLOSURE_CRATES: &[&str] = &[
    "addr2line",
    "adler2",
    "alloc",
    "cfg_if",
    "compiler_builtins",
    "core",
    "getopts",
    "gimli",
    "hashbrown",
    "libc",
    "memchr",
    "miniz_oxide",
    "object",
    "panic_abort",
    "panic_unwind",
    "proc_macro",
    "profiler_builtins",
    "rustc_demangle",
    "rustc_literal_escaper",
    "rustc_std_workspace_alloc",
    "rustc_std_workspace_core",
    "rustc_std_workspace_std",
    "std",
    "std_detect",
    "sysroot",
    "test",
    "unwind",
];

// Unlike registry dependencies that can also happen to have names such as
// `object`, these crates prove that Cargo's current stream contains a
// `-Zbuild-std` graph. Their package and source identities must both point into
// the selected toolchain's rust-src library tree.
const BUILD_STD_ANCHOR_CRATES: &[&str] = &[
    "alloc",
    "compiler_builtins",
    "core",
    "panic_abort",
    "panic_unwind",
    "proc_macro",
    "profiler_builtins",
    "std",
    "std_detect",
    "sysroot",
    "test",
    "unwind",
];

#[derive(Default)]
pub(crate) struct CargoOutputNotes {
    unused_patch_diagnostics: usize,
    build_std_future_incompat: usize,
}

pub(crate) fn run_cargo_command(cmd: &mut Command, label: &str) -> Result<(), String> {
    let output = cmd
        .output()
        .map_err(|err| format!("{label} failed to start: {err}"))?;
    let notes = write_filtered_cargo_output(label, &output.stdout, &output.stderr)?;
    print_cargo_output_notes(label, &notes);
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {}", output.status))
    }
}

pub(crate) fn run_cargo_rustc_command(
    cmd: &mut Command,
    label: &str,
    deps_dir: &Path,
) -> Result<CargoBuildArtifacts, String> {
    let output = cmd
        .output()
        .map_err(|err| format!("{label} failed to start: {err}"))?;

    let (mut artifacts, rendered_stdout) = parse_cargo_rustc_stdout(&output.stdout, deps_dir);
    // Cargo's dep-info paths are relative to the command's working directory
    // when rustc did not emit absolute paths. Preserve that identity before
    // the caller starts working with the cached input set.
    let process_dir = std::env::current_dir().map_err(io_string)?;
    let current_dir = cmd
        .get_current_dir()
        .map(|path| process_dir.join(path))
        .unwrap_or(process_dir);
    resolve_artifact_paths(&mut artifacts, &current_dir);

    io::stdout()
        .write_all(rendered_stdout.as_bytes())
        .map_err(io_string)?;
    let notes = write_filtered_cargo_output(label, &[], &output.stderr)?;
    print_cargo_output_notes(label, &notes);

    if output.status.success() {
        Ok(artifacts)
    } else {
        Err(format!("{label} failed with status {}", output.status))
    }
}

fn resolve_artifact_paths(artifacts: &mut CargoBuildArtifacts, current_dir: &Path) {
    for path in artifacts
        .input_files
        .iter_mut()
        .chain(artifacts.package_manifests.values_mut())
        .chain(
            artifacts
                .build_script_outputs
                .iter_mut()
                .map(|script| &mut script.path),
        )
    {
        if path.is_relative() {
            *path = current_dir.join(&*path);
        }
    }
    artifacts.input_files.sort();
    artifacts.input_files.dedup();
}

fn parse_cargo_rustc_stdout(stdout: &[u8], deps_dir: &Path) -> (CargoBuildArtifacts, String) {
    let mut artifacts = CargoBuildArtifacts::default();
    let mut sysroot_candidates = Vec::new();
    let mut target_metadata_candidates = Vec::new();
    let mut target_archive_candidates = Vec::new();
    let mut dep_info_paths = HashMap::new();
    let mut saw_build_std_anchor = false;
    let mut rendered_stdout = String::new();

    for line in String::from_utf8_lossy(stdout).lines() {
        match serde_json::from_str::<CargoJsonMessage>(line) {
            Ok(message) => match message.reason.as_str() {
                "compiler-artifact" => {
                    if let Some((package_id, manifest_path)) = message
                        .package_id
                        .as_ref()
                        .zip(message.manifest_path.as_ref())
                    {
                        artifacts
                            .package_manifests
                            .insert(package_id.clone(), PathBuf::from(manifest_path));
                    }
                    let package_dir = message
                        .manifest_path
                        .as_deref()
                        .and_then(|manifest| Path::new(manifest).parent());
                    // Use only dep-info associated with authenticated Cargo
                    // artifacts. A persistent deps directory also contains
                    // stale graphs and must never be scanned for inputs.
                    for filename in &message.filenames {
                        for dep_info_path in artifact_dep_info_paths(Path::new(filename)) {
                            dep_info_paths
                                .insert(dep_info_path, package_dir.map(Path::to_path_buf));
                        }
                    }
                    if let Some(target) = &message.target {
                        artifacts.input_files.push(resolve_dep_info_input(
                            PathBuf::from(&target.src_path),
                            package_dir,
                        ));
                    }
                    let target_artifact_in_deps = message.filenames.iter().any(|filename| {
                        let path = Path::new(filename);
                        path.parent().is_some_and(|parent| parent == deps_dir)
                            && matches!(
                                path.extension().and_then(|extension| extension.to_str()),
                                Some("rlib" | "rmeta")
                            )
                    });
                    if target_artifact_in_deps
                        && message
                            .package_id
                            .as_deref()
                            .zip(message.target.as_ref())
                            .is_some_and(|(package_id, target)| {
                                is_build_std_anchor(package_id, target)
                            })
                    {
                        saw_build_std_anchor = true;
                    }

                    for filename in &message.filenames {
                        let path = PathBuf::from(filename);
                        if !path.parent().is_some_and(|parent| parent == deps_dir) {
                            continue;
                        }

                        let Some(package_id) = message.package_id.as_ref() else {
                            continue;
                        };
                        let Some(target) = message.target.as_ref() else {
                            continue;
                        };
                        if path.extension().and_then(|extension| extension.to_str()) == Some("rlib")
                        {
                            target_archive_candidates.push(CargoNativeArchiveArtifact {
                                package_id: package_id.clone(),
                                crate_name: target.name.clone(),
                                source_path: PathBuf::from(&target.src_path),
                                path,
                            });
                            continue;
                        }
                        if path.extension().and_then(|extension| extension.to_str())
                            != Some("rmeta")
                        {
                            continue;
                        }
                        let artifact = CargoTargetMetadataArtifact {
                            package_id: package_id.clone(),
                            crate_name: target.name.clone(),
                            source_path: PathBuf::from(&target.src_path),
                            path,
                        };
                        if is_sysroot_closure_crate(&artifact.crate_name) {
                            sysroot_candidates.push(artifact.clone());
                        }
                        target_metadata_candidates.push(artifact);
                    }
                }
                "compiler-message" => {
                    if let Some(rendered) = message.message.and_then(|message| message.rendered) {
                        rendered_stdout.push_str(&rendered);
                        if !rendered.ends_with('\n') {
                            rendered_stdout.push('\n');
                        }
                    }
                }
                "build-script-executed" => {
                    if let Some((package_id, out_dir)) = message.package_id.zip(message.out_dir) {
                        if let Some(parent) = Path::new(&out_dir).parent() {
                            artifacts.build_script_outputs.push(CargoBuildScriptOutput {
                                package_id,
                                path: parent.join("output"),
                            });
                        }
                    }
                }
                _ => {}
            },
            Err(_) => {
                rendered_stdout.push_str(line);
                rendered_stdout.push('\n');
            }
        }
    }

    if saw_build_std_anchor {
        sysroot_candidates.sort_by(|left, right| {
            left.crate_name
                .cmp(&right.crate_name)
                .then_with(|| left.package_id.cmp(&right.package_id))
                .then_with(|| left.path.cmp(&right.path))
        });
        let mut seen_paths = HashSet::new();
        sysroot_candidates.retain(|artifact| seen_paths.insert(artifact.path.clone()));
        artifacts.sysroot_metadata = sysroot_candidates;
    }
    target_metadata_candidates.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.package_id.cmp(&right.package_id))
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut seen_paths = HashSet::new();
    target_metadata_candidates.retain(|artifact| seen_paths.insert(artifact.path.clone()));
    artifacts.target_metadata = target_metadata_candidates;
    target_archive_candidates.sort_by(|left, right| left.path.cmp(&right.path));
    // Keep conflicting identities for one path visible to the ownership
    // guard; repeated identical fresh messages are harmless.
    target_archive_candidates.dedup();
    artifacts.target_archives = target_archive_candidates;

    for (dep_info_path, package_dir) in dep_info_paths {
        if let Ok(dep_info) = std::fs::read_to_string(dep_info_path) {
            artifacts.input_files.extend(
                parse_rustc_dep_info(&dep_info)
                    .into_iter()
                    .map(|path| resolve_dep_info_input(path, package_dir.as_deref())),
            );
        }
    }
    artifacts.input_files.sort();
    artifacts.input_files.dedup();
    artifacts.build_script_outputs.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then_with(|| left.path.cmp(&right.path))
    });
    artifacts.build_script_outputs.dedup();

    (artifacts, rendered_stdout)
}

fn resolve_dep_info_input(path: PathBuf, package_dir: Option<&Path>) -> PathBuf {
    match package_dir {
        Some(package_dir) if path.is_relative() => package_dir.join(path),
        _ => path,
    }
}

fn artifact_dep_info_paths(artifact: &Path) -> Vec<PathBuf> {
    let mut paths = vec![artifact.with_extension("d")];
    // Cargo names library artifacts libfoo-HASH.rlib/rmeta/so, but rustc
    // writes their dependency files as foo-HASH.d. Binaries and build script
    // executables retain their complete stem.
    if let Some(stem) = artifact.file_stem().and_then(|stem| stem.to_str()) {
        if let Some(crate_stem) = stem.strip_prefix("lib") {
            paths.push(artifact.with_file_name(format!("{crate_stem}.d")));
        }
    }
    paths
}

/// Read the prerequisite side of rustc's Make-style dependency rules.
///
/// Empty target rules and rustc's env-dep comments are deliberately ignored.
/// Escaped whitespace, comments, colons, backslashes, dollar signs, and line
/// continuations retain the original input filenames.
fn parse_rustc_dep_info(dep_info: &str) -> Vec<PathBuf> {
    let dep_info = dep_info.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut paths = Vec::new();
    for line in dep_info.lines() {
        let mut escaped = false;
        let mut separator = None;
        let mut chars = line.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '#' {
                break;
            } else if ch == ':' && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) {
                separator = Some(index + 1);
                break;
            }
        }
        let Some(separator) = separator else {
            continue;
        };
        let mut path = String::new();
        let mut chars = line[separator..].chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    if let Some(next) = chars.next() {
                        path.push(next);
                    } else {
                        path.push('\\');
                    }
                }
                '$' if chars.peek() == Some(&'$') => {
                    chars.next();
                    path.push('$');
                }
                '#' => break,
                ch if ch.is_whitespace() => {
                    if !path.is_empty() {
                        paths.push(PathBuf::from(std::mem::take(&mut path)));
                    }
                }
                ch => path.push(ch),
            }
        }
        if !path.is_empty() {
            paths.push(PathBuf::from(path));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn is_sysroot_closure_crate(crate_name: &str) -> bool {
    SYSROOT_CLOSURE_CRATES.contains(&crate_name)
}

fn is_build_std_anchor(package_id: &str, target: &CargoJsonTarget) -> bool {
    BUILD_STD_ANCHOR_CRATES.contains(&target.name.as_str())
        && rust_src_library_relative_path(package_id)
            .zip(rust_src_library_relative_path(&target.src_path))
            .is_some_and(|(package_path, source_path)| {
                source_path == package_path
                    || source_path
                        .strip_prefix(package_path)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
}

fn rust_src_library_relative_path(value: &str) -> Option<&str> {
    const MARKER: &str = "/lib/rustlib/src/rust/library/";

    let value = value
        .strip_prefix("path+file://")
        .unwrap_or(value)
        .split('#')
        .next()
        .unwrap_or(value);
    value.split_once(MARKER).map(|(_, relative)| relative)
}

pub(crate) fn write_filtered_cargo_output(
    label: &str,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<CargoOutputNotes, String> {
    io::stdout().write_all(stdout).map_err(io_string)?;
    let stderr = String::from_utf8_lossy(stderr);
    let mut filtered = String::with_capacity(stderr.len());
    let mut notes = CargoOutputNotes::default();
    let mut skip_patch_help = false;
    let mut skip_future_incompat_note = false;

    for line in stderr.lines() {
        if line.starts_with("warning: patch `")
            && line.ends_with("` was not used in the crate graph")
        {
            notes.unused_patch_diagnostics += 1;
            skip_patch_help = true;
            continue;
        }

        if line.starts_with("warning: the following packages contain code that will be rejected by a future version of Rust: std v0.0.0 ")
        {
            notes.build_std_future_incompat += 1;
            skip_future_incompat_note = true;
            continue;
        }

        if skip_patch_help && line.starts_with("help: Check that the patched package version") {
            continue;
        }
        if skip_patch_help
            && (line.starts_with("      with the dependency requirements.")
                || line
                    .starts_with("      what is locked in the Cargo.lock file, run `cargo update`")
                || line
                    .starts_with("      version. This may also occur with an optional dependency"))
        {
            continue;
        }

        if skip_future_incompat_note
            && (line.starts_with("note: to see what the problems were, use the option")
                || line.starts_with("or run `cargo report future-incompatibilities"))
        {
            continue;
        }

        skip_patch_help = false;
        skip_future_incompat_note = false;
        filtered.push_str(line);
        filtered.push('\n');
    }

    io::stderr()
        .write_all(filtered.as_bytes())
        .map_err(|err| format!("{label} output write failed: {err}"))?;
    Ok(notes)
}

pub(crate) fn print_cargo_output_notes(label: &str, notes: &CargoOutputNotes) {
    if notes.unused_patch_diagnostics != 0 {
        eprintln!(
            "trueos-blueprint: note: suppressed {} unused source-overlay patch diagnostics during {label}",
            notes.unused_patch_diagnostics
        );
    }
    if notes.build_std_future_incompat != 0 {
        eprintln!(
            "trueos-blueprint: note: suppressed {} build-std future-incompat report for synthetic std during {label}",
            notes.build_std_future_incompat
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn cargo_artifact(
        package_id: &str,
        crate_name: &str,
        source_path: &str,
        filenames: &[&str],
    ) -> Value {
        json!({
            "reason": "compiler-artifact",
            "package_id": package_id,
            "target": {
                "name": crate_name,
                "src_path": source_path,
            },
            "filenames": filenames,
        })
    }

    fn cargo_stdout(messages: &[Value]) -> Vec<u8> {
        messages
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    }

    #[test]
    fn dep_info_reads_external_includes_and_make_escaped_paths() {
        let inputs = parse_rustc_dep_info(concat!(
            "# env-dep:IGNORED=not-a-filename\n",
            "target/app.d target/libapp.rlib: src/lib.rs ../outside/schema.json \\\n",
            "  assets/a\\ b.txt assets/hash\\#mark.txt assets/co\\:lon.txt ",
            "assets/dollar$$sign.txt assets/back\\\\slash.txt\n",
            "src/lib.rs:\n",
            "target/libapp.rmeta: src/lib.rs # comments are not inputs\n",
        ));
        assert_eq!(
            inputs,
            vec![
                PathBuf::from("../outside/schema.json"),
                PathBuf::from("assets/a b.txt"),
                PathBuf::from("assets/back\\slash.txt"),
                PathBuf::from("assets/co:lon.txt"),
                PathBuf::from("assets/dollar$sign.txt"),
                PathBuf::from("assets/hash#mark.txt"),
                PathBuf::from("src/lib.rs"),
            ]
        );
    }

    #[test]
    fn collects_only_current_artifact_dep_info_and_build_script_outputs() {
        static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = std::env::temp_dir().join(format!(
            "trueos-blueprint-cargo-inputs-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let deps_dir = temp.join("target/deps");
        let host_dir = temp.join("host/deps");
        std::fs::create_dir_all(&deps_dir).unwrap();
        std::fs::create_dir_all(&host_dir).unwrap();
        let rmeta = deps_dir.join("libapp-current.rmeta");
        let proc_macro = host_dir.join("libderive-current.so");
        std::fs::write(
            deps_dir.join("app-current.d"),
            "app.rmeta: src/lib.rs ../external/include.json\n",
        )
        .unwrap();
        std::fs::write(
            deps_dir.join("app-stale.d"),
            "app.rmeta: /stale/should-not-be-included.rs\n",
        )
        .unwrap();
        std::fs::write(
            host_dir.join("derive-current.d"),
            "derive.so: /host/proc-macro/src/lib.rs\n",
        )
        .unwrap();
        let script_out_dir = temp.join("target/build/app-current/out");
        let script = json!({
            "reason": "build-script-executed",
            "package_id": "path+file:///app#0.1.0",
            "out_dir": script_out_dir.to_str().unwrap(),
        });
        let package_dir = Path::new("/packages/app");
        let mut app = cargo_artifact(
            "path+file:///app#0.1.0",
            "app",
            "src/lib.rs",
            &[rmeta.to_str().unwrap()],
        );
        app["manifest_path"] = json!(package_dir.join("Cargo.toml"));
        let stdout = cargo_stdout(&[
            app,
            cargo_artifact(
                "path+file:///derive#0.1.0",
                "derive",
                "/host/proc-macro/src/lib.rs",
                &[proc_macro.to_str().unwrap()],
            ),
            script.clone(),
            script,
        ]);
        let (artifacts, rendered) = parse_cargo_rustc_stdout(&stdout, &deps_dir);
        assert!(rendered.is_empty());
        assert_eq!(
            artifacts.input_files,
            vec![
                PathBuf::from("/host/proc-macro/src/lib.rs"),
                package_dir.join("../external/include.json"),
                package_dir.join("src/lib.rs"),
            ]
        );
        assert_eq!(
            artifacts.build_script_outputs,
            vec![CargoBuildScriptOutput {
                package_id: "path+file:///app#0.1.0".to_string(),
                path: script_out_dir.parent().unwrap().join("output"),
            }]
        );
        assert_eq!(
            artifacts.package_manifests,
            BTreeMap::from([(
                "path+file:///app#0.1.0".to_string(),
                package_dir.join("Cargo.toml"),
            )])
        );
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn resolves_relative_package_manifests_against_cargo_working_directory() {
        let mut artifact = cargo_artifact(
            "path+file:///package#0.1.0",
            "package",
            "src/lib.rs",
            &["/target/libpackage.rmeta"],
        );
        artifact["manifest_path"] = json!("package/Cargo.toml");
        let (mut artifacts, _) =
            parse_cargo_rustc_stdout(&cargo_stdout(&[artifact]), Path::new("/target"));
        resolve_artifact_paths(&mut artifacts, Path::new("/cargo-cwd"));
        assert_eq!(
            artifacts
                .package_manifests
                .get("path+file:///package#0.1.0"),
            Some(&PathBuf::from("/cargo-cwd/package/Cargo.toml"))
        );
        assert_eq!(
            artifacts.input_files,
            vec![PathBuf::from("/cargo-cwd/package/src/lib.rs")]
        );
    }

    #[test]
    fn collects_sorted_deduplicated_current_invocation_sysroot_metadata() {
        let deps_dir = Path::new("/tmp/trueos-target/x86_64-unknown-trueos/release/deps");
        let object_rmeta = deps_dir.join("libobject-3333333333333333.rmeta");
        let object_rlib = deps_dir.join("libobject-3333333333333333.rlib");
        let std_rmeta = deps_dir.join("libstd-2222222222222222.rmeta");
        let std_rlib = deps_dir.join("libstd-2222222222222222.rlib");
        let libc_rmeta = deps_dir.join("liblibc-1111111111111111.rmeta");
        let libc_rlib = deps_dir.join("liblibc-1111111111111111.rlib");
        let app_rmeta = deps_dir.join("libapplication-4444444444444444.rmeta");
        let stale_object_rmeta =
            Path::new("/tmp/other-target/deps/libobject-0000000000000000.rmeta");
        let vendor_libc_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../TRUEOS-Buildins/vendor/libc-0.2.186");
        let vendor_libc = vendor_libc_root.join("src/lib.rs");
        let vendor_libc_package_id =
            format!("path+file://{}#libc@0.2.186", vendor_libc_root.display());

        // The registry dependency deliberately precedes the build-std anchor:
        // collection is gated over the complete current invocation, not JSON
        // message order.
        let stdout = cargo_stdout(&[
            cargo_artifact(
                "registry+https://github.com/rust-lang/crates.io-index#object@0.37.3",
                "object",
                "/home/test/.cargo/registry/src/object-0.37.3/src/lib.rs",
                &[
                    object_rmeta.to_str().unwrap(),
                    object_rlib.to_str().unwrap(),
                    stale_object_rmeta.to_str().unwrap(),
                ],
            ),
            cargo_artifact(
                "path+file:///opt/rust/lib/rustlib/src/rust/library/std#0.0.0",
                "std",
                "/opt/rust/lib/rustlib/src/rust/library/std/src/lib.rs",
                &[std_rmeta.to_str().unwrap(), std_rlib.to_str().unwrap()],
            ),
            cargo_artifact(
                &vendor_libc_package_id,
                "libc",
                vendor_libc.to_str().unwrap(),
                &[libc_rmeta.to_str().unwrap(), libc_rlib.to_str().unwrap()],
            ),
            // Cargo can repeat fresh artifact messages. Exact paths appear
            // once in the exported metadata set.
            cargo_artifact(
                &vendor_libc_package_id,
                "libc",
                vendor_libc.to_str().unwrap(),
                &[libc_rmeta.to_str().unwrap(), libc_rlib.to_str().unwrap()],
            ),
            cargo_artifact(
                "path+file:///tmp/application#0.1.0",
                "application",
                "/tmp/application/src/lib.rs",
                &[app_rmeta.to_str().unwrap()],
            ),
        ]);

        let (artifacts, rendered) = parse_cargo_rustc_stdout(&stdout, deps_dir);

        assert!(rendered.is_empty());
        assert_eq!(
            artifacts
                .sysroot_metadata
                .iter()
                .map(|artifact| artifact.path.as_path())
                .collect::<Vec<_>>(),
            vec![
                libc_rmeta.as_path(),
                object_rmeta.as_path(),
                std_rmeta.as_path(),
            ]
        );
        assert_eq!(artifacts.sysroot_metadata[0].crate_name, "libc");
        assert_eq!(
            artifacts.sysroot_metadata[0].package_id,
            vendor_libc_package_id
        );
        assert_eq!(
            artifacts.sysroot_metadata[0].source_path.as_path(),
            vendor_libc.as_path()
        );
        assert_eq!(
            artifacts
                .target_metadata
                .iter()
                .map(|artifact| artifact.path.as_path())
                .collect::<Vec<_>>(),
            vec![
                app_rmeta.as_path(),
                libc_rmeta.as_path(),
                object_rmeta.as_path(),
                std_rmeta.as_path(),
            ]
        );
        assert_eq!(
            artifacts
                .target_archives
                .iter()
                .map(|artifact| artifact.path.as_path())
                .collect::<Vec<_>>(),
            vec![
                libc_rlib.as_path(),
                object_rlib.as_path(),
                std_rlib.as_path()
            ]
        );
        assert_eq!(
            artifacts.target_archives[0].package_id,
            vendor_libc_package_id
        );
        assert_eq!(artifacts.target_archives[0].source_path, vendor_libc);
    }

    #[test]
    fn shuffled_panic_runtime_artifacts_are_stable_metadata_only() {
        let deps_dir = Path::new("/tmp/trueos-target/release/deps");
        let panic_abort_rmeta = deps_dir.join("libpanic_abort-aaaaaaaaaaaaaaaa.rmeta");
        let panic_abort_rlib = deps_dir.join("libpanic_abort-aaaaaaaaaaaaaaaa.rlib");
        let panic_unwind_rmeta = deps_dir.join("libpanic_unwind-bbbbbbbbbbbbbbbb.rmeta");
        let panic_unwind_rlib = deps_dir.join("libpanic_unwind-bbbbbbbbbbbbbbbb.rlib");
        let panic_abort = cargo_artifact(
            "path+file:///opt/rust/lib/rustlib/src/rust/library/panic_abort#0.0.0",
            "panic_abort",
            "/opt/rust/lib/rustlib/src/rust/library/panic_abort/src/lib.rs",
            &[
                panic_abort_rmeta.to_str().unwrap(),
                panic_abort_rlib.to_str().unwrap(),
            ],
        );
        let panic_unwind = cargo_artifact(
            "path+file:///opt/rust/lib/rustlib/src/rust/library/panic_unwind#0.0.0",
            "panic_unwind",
            "/opt/rust/lib/rustlib/src/rust/library/panic_unwind/src/lib.rs",
            &[
                panic_unwind_rmeta.to_str().unwrap(),
                panic_unwind_rlib.to_str().unwrap(),
            ],
        );

        let (forward, _) = parse_cargo_rustc_stdout(
            &cargo_stdout(&[panic_abort.clone(), panic_unwind.clone()]),
            deps_dir,
        );
        let (reversed, _) =
            parse_cargo_rustc_stdout(&cargo_stdout(&[panic_unwind, panic_abort]), deps_dir);

        let metadata_paths = |artifacts: &CargoBuildArtifacts| {
            artifacts
                .sysroot_metadata
                .iter()
                .map(|artifact| artifact.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(metadata_paths(&forward), metadata_paths(&reversed));
        assert_eq!(
            metadata_paths(&forward),
            vec![panic_abort_rmeta, panic_unwind_rmeta]
        );
    }

    #[test]
    fn ignores_known_names_without_a_rust_src_build_std_anchor() {
        let deps_dir = Path::new("/tmp/trueos-target/release/deps");
        let false_std = deps_dir.join("libstd-1111111111111111.rmeta");
        let object = deps_dir.join("libobject-2222222222222222.rmeta");
        let stdout = cargo_stdout(&[
            cargo_artifact(
                "registry+https://example.invalid/index#std@0.1.0",
                "std",
                "/tmp/application-named-std/src/lib.rs",
                &[false_std.to_str().unwrap()],
            ),
            cargo_artifact(
                "registry+https://github.com/rust-lang/crates.io-index#object@0.37.3",
                "object",
                "/home/test/.cargo/registry/src/object-0.37.3/src/lib.rs",
                &[object.to_str().unwrap()],
            ),
        ]);

        let (artifacts, _) = parse_cargo_rustc_stdout(&stdout, deps_dir);

        assert!(artifacts.sysroot_metadata.is_empty());
        assert_eq!(
            artifacts
                .target_metadata
                .iter()
                .map(|artifact| artifact.path.as_path())
                .collect::<Vec<_>>(),
            vec![object.as_path(), false_std.as_path()]
        );
    }

    #[test]
    fn build_std_anchor_must_emit_into_the_exact_deps_directory() {
        let deps_dir = Path::new("/tmp/trueos-target/release/deps");
        let other_core = "/tmp/other-target/deps/libcore-1111111111111111.rmeta";
        let object = deps_dir.join("libobject-2222222222222222.rmeta");
        let stdout = cargo_stdout(&[
            cargo_artifact(
                "path+file:///opt/rust/lib/rustlib/src/rust/library/core#0.0.0",
                "core",
                "/opt/rust/lib/rustlib/src/rust/library/core/src/lib.rs",
                &[other_core],
            ),
            cargo_artifact(
                "registry+https://github.com/rust-lang/crates.io-index#object@0.37.3",
                "object",
                "/home/test/.cargo/registry/src/object-0.37.3/src/lib.rs",
                &[object.to_str().unwrap()],
            ),
        ]);

        let (artifacts, _) = parse_cargo_rustc_stdout(&stdout, deps_dir);

        assert!(artifacts.sysroot_metadata.is_empty());
    }

    #[test]
    fn build_std_anchor_couples_package_and_source_identity() {
        let matching = CargoJsonTarget {
            name: "core".to_owned(),
            src_path: "/opt/rust/lib/rustlib/src/rust/library/core/src/lib.rs".to_owned(),
        };
        let mismatched = CargoJsonTarget {
            name: "core".to_owned(),
            src_path: "/opt/rust/lib/rustlib/src/rust/library/alloc/src/lib.rs".to_owned(),
        };
        let non_anchor = CargoJsonTarget {
            name: "object".to_owned(),
            src_path: "/opt/rust/lib/rustlib/src/rust/library/vendor/object/src/lib.rs".to_owned(),
        };
        let package_id = "path+file:///opt/rust/lib/rustlib/src/rust/library/core#0.0.0";

        assert!(is_build_std_anchor(package_id, &matching));
        assert!(!is_build_std_anchor(package_id, &mismatched));
        assert!(!is_build_std_anchor(package_id, &non_anchor));
    }

    #[test]
    fn preserves_rendered_diagnostics_and_non_json_output() {
        let diagnostic = json!({
            "reason": "compiler-message",
            "message": {
                "rendered": "warning: rendered diagnostic"
            }
        });
        let mut stdout = cargo_stdout(&[diagnostic]);
        stdout.extend_from_slice(b"\nplain cargo output\n");

        let (_, rendered) = parse_cargo_rustc_stdout(&stdout, Path::new("/tmp/deps"));

        assert_eq!(
            rendered,
            "warning: rendered diagnostic\nplain cargo output\n"
        );
    }
}
