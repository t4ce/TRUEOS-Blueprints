//! Source-derived native ownership folds. Response files keep the `.rlink`
//! order and original archives intact, including duplicate archive members.
//! Merging archives changed live section contents in the Codex experiment
//! without reducing allocated bytes, so ownership must not regroup inputs.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cargo_output::{
    CargoNativeArchiveArtifact, print_cargo_output_notes, write_filtered_cargo_output,
};
use crate::io_string;

#[derive(Deserialize)]
struct UnitGraph {
    version: u32,
    units: Vec<Unit>,
    roots: Vec<usize>,
}

#[derive(Deserialize, Serialize)]
struct Unit {
    pkg_id: String,
    target: Target,
    platform: Option<String>,
    mode: String,
    #[serde(default)]
    is_std: bool,
    features: Vec<String>,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize, Serialize)]
struct Target {
    name: String,
    src_path: PathBuf,
    kind: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Dependency {
    index: usize,
    #[serde(default)]
    noprelude: bool,
}

pub(crate) struct NativeFoldPlan {
    graph: UnitGraph,
    owner: usize,
    active: BTreeSet<usize>,
    exclusive: BTreeSet<usize>,
}

/// Copy the configured Cargo invocation before rustc-only trailing arguments
/// are added. Planning is locked/offline and does not compile. All environment,
/// source-overlay configs, target, profile, features and build-std flags remain
/// identical to the subsequent build.
pub(crate) fn capture_plan(
    cargo_rustc: &Command,
    work_dir: &Path,
    owner_crate: &str,
) -> Result<NativeFoldPlan, String> {
    ownership_plan(capture_graph(cargo_rustc, work_dir)?, owner_crate)
}

/// Retain the exact staged build plan for diagnostics without contracting its
/// graph or assigning ownership bundles to linker inputs.
pub(crate) fn capture_unit_graph(cargo_rustc: &Command, work_dir: &Path) -> Result<(), String> {
    capture_graph(cargo_rustc, work_dir).map(|_| ())
}

fn capture_graph(cargo_rustc: &Command, work_dir: &Path) -> Result<UnitGraph, String> {
    let mut plan = planning_command(cargo_rustc)?;
    let output = plan
        .output()
        .map_err(|err| format!("native ownership cargo plan failed to start: {err}"))?;
    let notes = write_filtered_cargo_output("native ownership cargo plan", &[], &output.stderr)?;
    print_cargo_output_notes("native ownership cargo plan", &notes);
    if !output.status.success() {
        return Err(format!(
            "native ownership cargo plan failed with status {}",
            output.status
        ));
    }
    let graph: UnitGraph = serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("invalid native ownership unit graph: {err}"))?;
    fs::write(work_dir.join("native-unit-graph.json"), &output.stdout).map_err(io_string)?;
    Ok(graph)
}

fn planning_command(cargo_rustc: &Command) -> Result<Command, String> {
    let mut plan = Command::new(cargo_rustc.get_program());
    if let Some(dir) = cargo_rustc.get_current_dir() {
        plan.current_dir(dir);
    }
    for (key, value) in cargo_rustc.get_envs() {
        if let Some(value) = value {
            plan.env(key, value);
        } else {
            plan.env_remove(key);
        }
    }
    let arguments: Vec<_> = cargo_rustc.get_args().collect();
    let subcommand = arguments
        .iter()
        .position(|argument| *argument == "rustc")
        .ok_or_else(|| "native ownership planning requires a cargo rustc invocation".to_string())?;
    // The rustup fallback prefixes Cargo with +<pinned-toolchain>. Preserve
    // that selector as well as any global options preceding the subcommand.
    plan.args(&arguments[..subcommand]).arg("build");
    for argument in &arguments[subcommand + 1..] {
        if *argument == "--" {
            return Err("native ownership planning must precede rustc-only arguments".into());
        }
        if *argument != "--message-format=json-render-diagnostics" {
            plan.arg(argument);
        }
    }
    plan.args([
        "--unit-graph",
        "-Z",
        "unstable-options",
        "--locked",
        "--offline",
    ]);
    Ok(plan)
}

fn ownership_plan(graph: UnitGraph, owner_crate: &str) -> Result<NativeFoldPlan, String> {
    if graph.version != 1 || graph.roots.is_empty() {
        return Err("unsupported or rootless native ownership unit graph".into());
    }
    if graph.roots.iter().any(|&root| root >= graph.units.len())
        || graph.units.iter().any(|unit| {
            unit.dependencies
                .iter()
                .any(|edge| edge.index >= graph.units.len())
        })
    {
        return Err("native ownership unit graph has an invalid dependency index".into());
    }
    let target = graph.units[graph.roots[0]].platform.as_deref();
    if target.is_none()
        || graph
            .roots
            .iter()
            .any(|&root| graph.units[root].platform.as_deref() != target)
    {
        return Err("native ownership roots must use one explicit target platform".into());
    }
    let selected: BTreeSet<_> = graph
        .units
        .iter()
        .enumerate()
        .filter(|(_, unit)| {
            unit.platform.as_deref() == target
                && unit.mode == "build"
                && !unit.is_std
                && !unit
                    .target
                    .kind
                    .iter()
                    .any(|kind| kind == "custom-build" || kind == "proc-macro")
        })
        .map(|(index, _)| index)
        .collect();
    let edges: Vec<Vec<_>> = graph
        .units
        .iter()
        .map(|unit| {
            unit.dependencies
                .iter()
                .filter(|edge| !edge.noprelude && selected.contains(&edge.index))
                .map(|edge| edge.index)
                .collect()
        })
        .collect();
    let active = reachable(&graph.roots, &edges, None);
    let owners: Vec<_> = active
        .iter()
        .copied()
        .filter(|&index| graph.units[index].target.name == owner_crate)
        .collect();
    if owners.len() != 1 {
        return Err(format!(
            "native ownership expected one active {owner_crate} unit, found {}",
            owners.len()
        ));
    }
    let owner = owners[0];
    let without_owner = reachable(&graph.roots, &edges, Some(owner));
    let exclusive = active
        .difference(&without_owner)
        .copied()
        .collect::<BTreeSet<_>>();
    // A dominator fold has one legal inbound boundary. Check the invariant
    // explicitly so changes to graph selection cannot swallow shared units.
    for &parent in active.difference(&exclusive) {
        if edges[parent]
            .iter()
            .any(|&child| exclusive.contains(&child) && child != owner)
        {
            return Err("native ownership fold has an inbound edge below its owner".into());
        }
    }
    Ok(NativeFoldPlan {
        graph,
        owner,
        active,
        exclusive,
    })
}

fn reachable(roots: &[usize], edges: &[Vec<usize>], blocked: Option<usize>) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut todo = roots.to_vec();
    while let Some(index) = todo.pop() {
        if Some(index) == blocked || !found.insert(index) {
            continue;
        }
        todo.extend(edges[index].iter().copied());
    }
    found
}

#[derive(Serialize)]
struct SelectedArchive<'a> {
    input_index: usize,
    path: &'a Path,
    package_id: &'a str,
    crate_name: &'a str,
    source_path: &'a Path,
    owned: bool,
}

#[derive(Serialize)]
struct ResponseRun {
    response_file: PathBuf,
    first_input: usize,
    past_last_input: usize,
}

#[derive(Serialize)]
struct FoldManifest<'a> {
    version: u32,
    method: &'a str,
    root_rlink: &'a Path,
    owner: &'a Unit,
    active_units: usize,
    exclusive_units: usize,
    folded_units: usize,
    members: Vec<&'a Unit>,
    archives: Vec<SelectedArchive<'a>>,
    response_runs: Vec<ResponseRun>,
}

/// Replace only contiguous owned runs by response files. Expanding the result
/// yields exactly the original `.rlink` archive list in exactly the same order,
/// inside the caller's existing linker group. No archive is extracted, renamed,
/// rebuilt or repeated, so duplicate object member names stay distinct.
pub(crate) fn write_link_inputs(
    plan: &NativeFoldPlan,
    rlibs: &[PathBuf],
    artifacts: &[CargoNativeArchiveArtifact],
    root_rlink: &Path,
    work_dir: &Path,
) -> Result<Vec<OsString>, String> {
    let mut identities = BTreeMap::new();
    for artifact in artifacts {
        if let Some(previous) = identities.insert(&artifact.path, artifact) {
            if previous != artifact {
                return Err(format!(
                    "Cargo reported conflicting identities for {}",
                    artifact.path.display()
                ));
            }
        }
    }
    let members: BTreeSet<_> = plan
        .exclusive
        .iter()
        .map(|&index| {
            let unit = &plan.graph.units[index];
            (&unit.pkg_id, &unit.target.name, &unit.target.src_path)
        })
        .collect();
    let mut archives = Vec::with_capacity(rlibs.len());
    for (input_index, path) in rlibs.iter().enumerate() {
        let artifact = identities.get(path).ok_or_else(|| {
            format!(
                "selected native archive {} is absent from the current Cargo artifact stream",
                path.display()
            )
        })?;
        let owned = members.contains(&(
            &artifact.package_id,
            &artifact.crate_name,
            &artifact.source_path,
        ));
        archives.push(SelectedArchive {
            input_index,
            path,
            package_id: &artifact.package_id,
            crate_name: &artifact.crate_name,
            source_path: &artifact.source_path,
            owned,
        });
    }
    let owner_unit = &plan.graph.units[plan.owner];
    if !archives.iter().any(|archive| {
        archive.owned
            && archive.package_id == owner_unit.pkg_id
            && archive.crate_name == owner_unit.target.name
            && archive.source_path == owner_unit.target.src_path
    }) {
        return Err("native ownership owner is absent from the authenticated link closure".into());
    }

    let mut inputs = Vec::new();
    let mut response_runs = Vec::new();
    let mut cursor = 0;
    while cursor < archives.len() {
        if !archives[cursor].owned {
            inputs.push(archives[cursor].path.as_os_str().to_owned());
            cursor += 1;
            continue;
        }
        let first = cursor;
        let mut contents = String::new();
        while cursor < archives.len() && archives[cursor].owned {
            contents.push_str(&response_path(archives[cursor].path)?);
            cursor += 1;
        }
        let response_file =
            work_dir.join(format!("native-network-owner-{}.rsp", response_runs.len()));
        fs::write(&response_file, contents).map_err(io_string)?;
        let mut argument = OsString::from("@");
        argument.push(&response_file);
        inputs.push(argument);
        response_runs.push(ResponseRun {
            response_file,
            first_input: first,
            past_last_input: cursor,
        });
    }
    let manifest = FoldManifest {
        version: 1,
        method: "exclusive source ownership; contiguous response runs preserve native input order and archives",
        root_rlink,
        owner: owner_unit,
        active_units: plan.active.len(),
        exclusive_units: plan.exclusive.len(),
        folded_units: plan.active.len() - plan.exclusive.len() + 1,
        members: plan
            .exclusive
            .iter()
            .map(|&index| &plan.graph.units[index])
            .collect(),
        archives,
        response_runs,
    };
    fs::write(
        work_dir.join("native-folds.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(io_string)?;
    println!(
        "trueos-blueprint: native network ownership: {} units, {} selected archives, {} ordered response runs",
        manifest.exclusive_units,
        manifest
            .archives
            .iter()
            .filter(|archive| archive.owned)
            .count(),
        manifest.response_runs.len(),
    );
    Ok(inputs)
}

fn response_path(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or_else(|| {
        format!(
            "native response archive path is not UTF-8: {}",
            path.display()
        )
    })?;
    if !path.is_absolute() || value.contains(['\n', '\r', '\0']) {
        return Err(format!(
            "invalid native response archive path: {}",
            path.display()
        ));
    }
    Ok(format!(
        "\"{}\"\n",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unit(name: &str, package: &str, dependencies: &[usize]) -> serde_json::Value {
        json!({
            "pkg_id": package,
            "target": {"name": name, "src_path": format!("/src/{package}/lib.rs"), "kind": ["lib"]},
            "platform": "trueos.json", "mode": "build", "features": ["feature-a"],
            "dependencies": dependencies.iter().map(|index| json!({"index": index})).collect::<Vec<_>>()
        })
    }

    fn fixture() -> UnitGraph {
        let mut host = unit("host_tool", "host-tool", &[]);
        host["platform"] = json!(null);
        let mut std = unit("std", "sysroot", &[]);
        std["is_std"] = json!(true);
        let mut root = unit("codex", "codex-cli", &[1, 4, 5, 6, 7, 8]);
        root["dependencies"][4]["noprelude"] = json!(true);
        serde_json::from_value(json!({
            "version": 1, "roots": [0],
            "units": [
                root,
                unit("codex_network_proxy", "network", &[2, 4]),
                unit("leaf", "leaf@1", &[3]),
                unit("deep_leaf", "deep", &[]),
                unit("shared_tls", "shared", &[]),
                unit("other_client", "other", &[4]),
                host, std,
                unit("leaf", "leaf@2", &[]),
            ]
        }))
        .unwrap()
    }

    fn temp_dir(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "native-fold-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn artifact(plan: &NativeFoldPlan, index: usize, path: PathBuf) -> CargoNativeArchiveArtifact {
        let unit = &plan.graph.units[index];
        CargoNativeArchiveArtifact {
            package_id: unit.pkg_id.clone(),
            crate_name: unit.target.name.clone(),
            source_path: unit.target.src_path.clone(),
            path,
        }
    }

    #[test]
    fn planning_command_keeps_toolchain_source_flags_profile_and_environment() {
        for prefix in [None, Some("+nightly-2026-07-10")] {
            let mut cargo = Command::new("cargo");
            if let Some(prefix) = prefix {
                cargo.arg(prefix);
            }
            cargo
                .args([
                    "rustc",
                    "--message-format=json-render-diagnostics",
                    "-Z",
                    "build-std=core,alloc,std,panic_abort",
                    "-Z",
                    "json-target-spec",
                    "--target",
                    "/target/trueos.json",
                    "--manifest-path",
                    "/stage/Cargo.toml",
                    "--config",
                    "patch.crates-io.helper.path=\"/source/helper\"",
                    "--no-default-features",
                    "--features",
                    "trueos-blueprint,trueos/default-global-allocator",
                    "--profile",
                    "bringup",
                    "--bin",
                    "codex",
                ])
                .current_dir("/stage")
                .env("RUSTC", "/archive/bin/rustc")
                .env("CARGO_TARGET_DIR", "/cache")
                .env("CARGO_ENCODED_RUSTFLAGS", "-C\u{1f}panic=abort")
                .env_remove("RUSTFLAGS");
            let plan = planning_command(&cargo).unwrap();
            assert_eq!(plan.get_program(), cargo.get_program());
            assert_eq!(plan.get_current_dir(), cargo.get_current_dir());
            assert_eq!(
                plan.get_envs().collect::<Vec<_>>(),
                cargo.get_envs().collect::<Vec<_>>()
            );
            let expected: Vec<OsString> = cargo
                .get_args()
                .filter(|arg| *arg != "--message-format=json-render-diagnostics")
                .map(|arg| {
                    if arg == "rustc" {
                        OsString::from("build")
                    } else {
                        arg.to_owned()
                    }
                })
                .chain(
                    [
                        "--unit-graph",
                        "-Z",
                        "unstable-options",
                        "--locked",
                        "--offline",
                    ]
                    .map(OsString::from),
                )
                .collect();
            assert_eq!(
                plan.get_args().collect::<Vec<_>>(),
                expected.iter().map(OsString::as_os_str).collect::<Vec<_>>()
            );
            cargo.args(["--", "-Zno-link", "--emit=obj"]);
            assert!(
                planning_command(&cargo)
                    .unwrap_err()
                    .contains("must precede rustc-only")
            );
        }
    }

    #[test]
    fn ownership_excludes_shared_host_sysroot_and_same_name_other_version() {
        let plan = ownership_plan(fixture(), "codex_network_proxy").unwrap();
        assert_eq!(plan.active, BTreeSet::from([0, 1, 2, 3, 4, 5, 8]));
        assert_eq!(plan.exclusive, BTreeSet::from([1, 2, 3]));

        let mut graph = fixture();
        graph.units[0].dependencies[0].index = graph.units.len();
        assert!(
            ownership_plan(graph, "codex_network_proxy")
                .err()
                .unwrap()
                .contains("invalid dependency")
        );

        let mut graph = fixture();
        graph.units[5].target.name = "codex_network_proxy".into();
        assert!(
            ownership_plan(graph, "codex_network_proxy")
                .err()
                .unwrap()
                .contains("found 2")
        );
    }

    #[test]
    fn response_runs_preserve_native_input_order_and_exact_source_identities() {
        let plan = ownership_plan(fixture(), "codex_network_proxy").unwrap();
        let temp = temp_dir("order");
        let archives: Vec<_> = [3, 4, 1, 2, 8]
            .into_iter()
            .map(|index| artifact(&plan, index, temp.join(format!("archive {index}.rlib"))))
            .collect();
        let paths: Vec<_> = archives
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect();
        let mut current_stream = archives.clone();
        // A stale/fresh but unselected owned archive cannot enter the link.
        current_stream.push(artifact(&plan, 3, temp.join("unselected.rlib")));
        let inputs = write_link_inputs(
            &plan,
            &paths,
            &current_stream,
            Path::new("root.rlink"),
            &temp,
        )
        .unwrap();
        assert_eq!(inputs.len(), 4);
        assert_eq!(inputs[1], paths[1]);
        assert_eq!(inputs[3], paths[4]);
        assert_eq!(
            fs::read_to_string(temp.join("native-network-owner-0.rsp")).unwrap(),
            response_path(&paths[0]).unwrap()
        );
        assert_eq!(
            fs::read_to_string(temp.join("native-network-owner-1.rsp")).unwrap(),
            format!(
                "{}{}",
                response_path(&paths[2]).unwrap(),
                response_path(&paths[3]).unwrap()
            )
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.join("native-folds.json")).unwrap()).unwrap();
        assert_eq!(manifest["archives"].as_array().unwrap().len(), 5);
        assert_eq!(manifest["archives"][4]["owned"], false);
        assert_eq!(manifest["archives"][3]["package_id"], "leaf@1");
        assert_eq!(manifest["archives"][4]["package_id"], "leaf@2");
        assert_eq!(manifest["response_runs"][1]["first_input"], 2);
        assert_eq!(manifest["response_runs"][1]["past_last_input"], 4);
        assert_eq!(
            response_path(Path::new("/a space/quote\"back\\.rlib")).unwrap(),
            "\"/a space/quote\\\"back\\\\.rlib\"\n"
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn ownership_refuses_missing_conflicting_and_wrong_source_artifacts() {
        let plan = ownership_plan(fixture(), "codex_network_proxy").unwrap();
        let temp = temp_dir("guard");
        let archive = artifact(&plan, 1, temp.join("owner.rlib"));
        let paths = vec![archive.path.clone()];
        let error =
            write_link_inputs(&plan, &paths, &[], Path::new("root.rlink"), &temp).unwrap_err();
        assert!(error.contains("absent from the current Cargo artifact stream"));
        let mut conflicting = archive.clone();
        conflicting.package_id = "unrelated".into();
        let error = write_link_inputs(
            &plan,
            &paths,
            &[archive.clone(), conflicting],
            Path::new("root.rlink"),
            &temp,
        )
        .unwrap_err();
        assert!(error.contains("conflicting identities"));
        let mut wrong_source = archive;
        wrong_source.source_path = PathBuf::from("/stale/lib.rs");
        let error = write_link_inputs(
            &plan,
            &paths,
            &[wrong_source],
            Path::new("root.rlink"),
            &temp,
        )
        .unwrap_err();
        assert!(error.contains("owner is absent"));
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn ordered_response_link_matches_original_with_duplicate_archive_member_names() {
        let plan = ownership_plan(fixture(), "codex_network_proxy").unwrap();
        let temp = temp_dir("native-link");
        let mut artifacts = Vec::new();
        for (index, source) in [
            (
                1,
                "extern int deep(void); int owner(void) { return deep(); } __attribute__((weak)) int choice(void) { return 1; }",
            ),
            (4, "int shared(void) { return 40; }"),
            (
                3,
                "int deep(void) { return 2; } __attribute__((weak)) int choice(void) { return 3; }",
            ),
        ] {
            let dir = temp.join(format!("member {index}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("member.c"), source).unwrap();
            crate::run_command(
                Command::new("cc")
                    .args(["-O2", "-ffunction-sections", "-fdata-sections", "-c"])
                    .arg(dir.join("member.c"))
                    .arg("-o")
                    .arg(dir.join("member.o")),
                "compile archive member",
            )
            .unwrap();
            let archive_path = temp.join(format!("native {index}.rlib"));
            let mut ar = crate::artifact::tool_command(&["llvm-ar", "ar"]).unwrap();
            crate::run_command(
                ar.arg("crs").arg(&archive_path).arg(dir.join("member.o")),
                "create fixture archive",
            )
            .unwrap();
            artifacts.push(artifact(&plan, index, archive_path));
        }
        fs::write(temp.join("root.c"), "extern int owner(void), shared(void), choice(void); int _start(void) { return owner() + shared() + choice(); }").unwrap();
        crate::run_command(
            Command::new("cc")
                .args(["-O2", "-ffunction-sections", "-c"])
                .arg(temp.join("root.c"))
                .arg("-o")
                .arg(temp.join("root.o")),
            "compile fixture root",
        )
        .unwrap();
        let paths: Vec<_> = artifacts
            .iter()
            .map(|artifact| artifact.path.clone())
            .collect();
        let folded =
            write_link_inputs(&plan, &paths, &artifacts, Path::new("root.rlink"), &temp).unwrap();
        fs::write(temp.join("coalesce.ld"), crate::REL_COALESCE_LINKER_SCRIPT).unwrap();
        for (name, inputs) in [
            (
                "original.o",
                paths
                    .iter()
                    .map(|path| path.as_os_str().to_owned())
                    .collect::<Vec<_>>(),
            ),
            ("folded.o", folded),
        ] {
            let mut ld = crate::artifact::tool_command(&["ld.lld", "rust-lld", "ld"]).unwrap();
            ld.args(["-r", "--gc-sections", "--undefined", "_start", "-T"])
                .arg(temp.join("coalesce.ld"))
                .arg("-o")
                .arg(temp.join(name))
                .arg(temp.join("root.o"))
                .arg("--start-group")
                .args(inputs)
                .arg("--end-group");
            crate::run_command(&mut ld, "link fixture").unwrap();
        }
        assert_eq!(
            fs::read(temp.join("original.o")).unwrap(),
            fs::read(temp.join("folded.o")).unwrap()
        );
        assert_ne!(
            crate::artifact::entry_hint_hex(&temp.join("folded.o")),
            "0000000000000000"
        );
        fs::remove_dir_all(temp).unwrap();
    }
}
