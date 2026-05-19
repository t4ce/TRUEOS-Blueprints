use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

mod artifact;
mod build_plan;
mod publish;

use artifact::{
    cargo_artifact_stem, collect_rlibs_for_object, entry_hint_hex, entry_symbol_name,
    latest_cargo_object, tool_command, write_blueprint,
};
use build_plan::{BuildFlavor, BuildSettings, BuildTarget, resolve_build_settings};
use publish::publish_dist_blueprints;

#[derive(Clone)]
struct ExampleSpec {
    name: String,
    required_features: Vec<String>,
}

struct PackageAppSpec {
    name: String,
    dir: PathBuf,
    manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct AppRegistry {
    apps: Vec<String>,
}

#[derive(Clone, Copy)]
enum CargoProfile {
    Dev,
    Release,
}

impl CargoProfile {
    fn target_subdir(self) -> &'static str {
        match self {
            CargoProfile::Dev => "debug",
            CargoProfile::Release => "release",
        }
    }

    fn label(self) -> &'static str {
        match self {
            CargoProfile::Dev => "dev",
            CargoProfile::Release => "release",
        }
    }
}

struct CratePatch {
    name: String,
    path: PathBuf,
}

#[derive(Default)]
struct CargoBuildArtifacts {
    rlibs: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct CargoJsonMessage {
    reason: String,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    message: Option<CargoJsonDiagnostic>,
}

#[derive(Deserialize)]
struct CargoJsonDiagnostic {
    rendered: Option<String>,
}

#[derive(Default)]
struct CargoOutputNotes {
    unused_patch_diagnostics: usize,
    build_std_future_incompat: usize,
}

const CARGO_CACHE_DIR_ENV: &str = "TRUEOS_BLUEPRINT_CARGO_CACHE_DIR";
const TARGET_SPEC_ENV: &str = "TRUEOS_BLUEPRINT_TARGET_SPEC";
const RUSTFLAGS_ENCODED_SEPARATOR: char = '\u{1f}';
const TRUEOS_CHECK_CFG_FLAG: &str = "--check-cfg=cfg(target_os,values(\"trueos\",\"zkvm\"))";
const BLUEPRINT_RUSTFLAGS: &[&str] = &[
    TRUEOS_CHECK_CFG_FLAG,
    "--cfg",
    "getrandom_backend=\"unsupported\"",
    "-A",
    "warnings",
];

fn main() {
    if let Err(err) = run() {
        eprintln!("trueos-blueprint: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (app_dir, requested_apps, cargo_profile) = parse_cli_args(&args)?;
    let app_dir = fs::canonicalize(&app_dir)
        .map_err(|err| format!("failed to resolve app dir {}: {err}", app_dir.display()))?;
    let manifest_path = app_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("missing Cargo.toml in {}", app_dir.display()));
    }

    if package_name(&manifest_path)? == "trueos-blueprint" {
        if requested_apps.is_empty() {
            let examples = example_specs(&manifest_path)?;
            let package_apps = package_app_specs(&app_dir)?;
            if examples.is_empty() && package_apps.is_empty() {
                return build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Package,
                    &[],
                    cargo_profile,
                );
            }

            for example in examples {
                build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Example(example.name),
                    &example.required_features,
                    cargo_profile,
                )?;
            }

            for package_app in package_apps {
                println!("trueos-blueprint: package app: {}", package_app.name);
                build_one_target_to(
                    &package_app.dir,
                    &package_app.manifest_path,
                    BuildTarget::Package,
                    &[],
                    &app_dir.join("dist"),
                    cargo_profile,
                )?;
            }

            publish_dist_blueprints(&app_dir.join("dist"))?;
            return Ok(());
        }

        for example_name in requested_apps {
            if let Ok(required_features) = example_required_features(&manifest_path, &example_name)
            {
                build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Example(example_name),
                    &required_features,
                    cargo_profile,
                )?;
                continue;
            }

            if let Some(package_app) = package_app_spec(&app_dir, &example_name)? {
                build_one_target_to(
                    &package_app.dir,
                    &package_app.manifest_path,
                    BuildTarget::Package,
                    &[],
                    &app_dir.join("dist"),
                    cargo_profile,
                )?;
                continue;
            }

            return Err(format!("unknown example or package `{example_name}`"));
        }
        return Ok(());
    }

    if !requested_apps.is_empty() {
        return Err("named apps are only supported from the trueos-blueprint root".to_string());
    }

    let build_target = BuildTarget::Package;
    let required_features = Vec::new();
    build_one_target(&app_dir, &manifest_path, build_target, &required_features, cargo_profile)
}

fn parse_cli_args(
    args: &[std::ffi::OsString],
) -> Result<(PathBuf, Vec<String>, CargoProfile), String> {
    let mut cargo_profile = CargoProfile::Release;
    let mut filtered_args = Vec::with_capacity(args.len());
    for arg in args {
        if arg == "--release" {
            cargo_profile = CargoProfile::Release;
        } else {
            filtered_args.push(arg.clone());
        }
    }

    if filtered_args.is_empty() {
        return Ok((PathBuf::from("."), Vec::new(), cargo_profile));
    }

    let first = PathBuf::from(&filtered_args[0]);
    if first.join("Cargo.toml").is_file() {
        if filtered_args.len() > 1 {
            return Err("directory mode does not accept app names".to_string());
        }
        return Ok((first, Vec::new(), cargo_profile));
    }

    let mut app_names = Vec::with_capacity(filtered_args.len());
    for arg in filtered_args {
        app_names.push(
            arg.into_string()
                .map_err(|_| "app name must be valid UTF-8".to_string())?,
        );
    }

    Ok((PathBuf::from("."), app_names, cargo_profile))
}

fn build_one_target(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: BuildTarget,
    required_features: &[String],
    cargo_profile: CargoProfile,
) -> Result<(), String> {
    build_one_target_to(
        app_dir,
        manifest_path,
        build_target,
        required_features,
        &app_dir.join("dist"),
        cargo_profile,
    )
}

fn build_one_target_to(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: BuildTarget,
    required_features: &[String],
    output_dir: &Path,
    cargo_profile: CargoProfile,
) -> Result<(), String> {
    let cargo_profile = if matches!(build_target, BuildTarget::Package) {
        package_blueprint_profile(manifest_path)?.unwrap_or(cargo_profile)
    } else {
        cargo_profile
    };
    let build_settings = resolve_build_settings(&app_dir, &manifest_path, &build_target)?;

    let output_name = match &build_target {
        BuildTarget::Package => package_name(&manifest_path)?,
        BuildTarget::Example(name) => name.clone(),
    };

    let packer_target_dir = app_dir.join("target").join("trueos-blueprint");
    let target_spec = cargo_target_spec_path(&default_target_spec(&app_dir)?, &packer_target_dir)?;
    let target_name = target_spec
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("bad target spec path: {}", target_spec.display()))?
        .to_string();
    let cargo_cache_root = cargo_cache_root(&app_dir, &packer_target_dir);
    let cargo_target_dir = cargo_cache_root
        .join(&target_name)
        .join(build_settings.flavor.cache_label());
    fs::create_dir_all(&cargo_target_dir).map_err(io_string)?;
    let target_dir = cargo_target_dir
        .join(&target_name)
        .join(cargo_profile.target_subdir());
    let deps_dir = target_dir.join("deps");

    let work_dir = workdir(app_dir, &packer_target_dir, &output_name)?;
    reset_dir(&work_dir)?;

    let source_overlay = source_overlay_patches(app_dir, manifest_path)?;
    let lock_mismatches = source_overlay_lock_mismatches(app_dir, &source_overlay)?;
    preflight_source_overlay_version_alignment(app_dir, manifest_path, &lock_mismatches)?;
    let staged_source_overlay = staged_source_overlay(&source_overlay, &work_dir);
    let cargo_manifest_path = staged_manifest_for_overlay(
        app_dir,
        manifest_path,
        &work_dir,
        &build_settings,
        &source_overlay,
        &lock_mismatches,
    )?
    .unwrap_or_else(|| manifest_path.to_path_buf());
    enforce_source_overlay_lock(&cargo_manifest_path, &staged_source_overlay)?;

    let mut cargo = Command::new("cargo");
    if let Some(manifest_dir) = cargo_manifest_path.parent() {
        cargo.current_dir(manifest_dir);
    }
    cargo
        .arg("+nightly")
        .arg("rustc")
        .arg("--message-format=json-render-diagnostics")
        .arg("-Z")
        .arg(match build_settings.flavor {
            BuildFlavor::TokioStd => "build-std=core,compiler_builtins,alloc,std,panic_abort",
            BuildFlavor::ThinNoStd => "build-std=core,compiler_builtins,alloc",
        })
        .arg("-Z")
        .arg("json-target-spec")
        .arg("--target")
        .arg(&target_spec)
        .arg("--manifest-path")
        .arg(&cargo_manifest_path);
    if !source_overlay.is_empty() {
        println!(
            "trueos-blueprint: source overlay crates: {}",
            source_overlay
                .iter()
                .map(|patch| format!("{}={}", patch.name, patch.path.display()))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    push_source_overlay_configs(&mut cargo, &staged_source_overlay);
    push_extra_rustflags(&mut cargo, BLUEPRINT_RUSTFLAGS);
    push_bindgen_clang_args(&mut cargo);
    push_trueos_cc_flags(&mut cargo);
    cargo.env("RUSTC_BOOTSTRAP_SYNTHETIC_TARGET", "1");
    cargo.env("CARGO_TARGET_DIR", &cargo_target_dir);
    let declared_features = manifest_declared_features(&cargo_manifest_path)?;
    let has_trueos_dependency = manifest_has_dependency(&cargo_manifest_path, "trueos")?;
    let mut extra_features = required_features.to_vec();
    for feature in &build_settings.extra_features {
        push_app_or_trueos_feature(
            &mut extra_features,
            feature,
            &declared_features,
            has_trueos_dependency,
        );
    }
    if !build_settings.has_global_allocator && has_trueos_dependency {
        push_feature(&mut extra_features, "trueos/default-global-allocator");
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd)
        && !build_settings.has_panic_handler
        && has_trueos_dependency
    {
        push_feature(&mut extra_features, "trueos/default-panic-handler");
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) || is_helix_app_dir(app_dir) {
        cargo.arg("--no-default-features");
    } else if build_settings.needs_tokio_net {
        push_app_or_trueos_feature(
            &mut extra_features,
            "tokio-net-probe",
            &declared_features,
            has_trueos_dependency,
        );
    }
    if !extra_features.is_empty() {
        cargo.arg("--features").arg(extra_features.join(","));
    }
    if matches!(cargo_profile, CargoProfile::Release) {
        cargo.arg("--release");
    }

    match &build_target {
        BuildTarget::Package => {
            if let Some(bin_name) = package_bin_name(&cargo_manifest_path)? {
                cargo.arg("--bin").arg(bin_name);
            }
        }
        BuildTarget::Example(name) => {
            cargo.arg("--example").arg(name);
        }
    };
    cargo.arg("--").arg("-Zno-link").arg("--emit=obj");

    println!("trueos-blueprint: cargo artifact profile: {}", cargo_profile.label());
    println!("trueos-blueprint: cargo artifact cache: {}", cargo_target_dir.display());
    let cargo_artifacts = run_cargo_rustc_command(&mut cargo, "cargo rustc", &deps_dir)?;

    if !deps_dir.is_dir() {
        return Err(format!("missing deps dir: {}", deps_dir.display()));
    }

    let app_obj = match &build_target {
        BuildTarget::Package => latest_cargo_object(&deps_dir, &cargo_artifact_stem(&output_name))?,
        BuildTarget::Example(name) => {
            latest_cargo_object(&target_dir.join("examples"), &cargo_artifact_stem(name))?
        }
    };
    let rlibs = if cargo_artifacts.rlibs.is_empty() {
        println!(
            "trueos-blueprint: note: cargo artifact stream yielded no target rlibs; falling back to legacy .rlink scrape"
        );
        collect_rlibs_for_object(&app_obj, &deps_dir)?
    } else {
        cargo_artifacts.rlibs
    };

    let linked = work_dir.join("module.o");
    let stripped = work_dir.join("module.stripped.o");
    let entry_symbol = entry_symbol_name(&app_obj);
    let link_app_obj = if let Some(symbol) = &entry_symbol {
        let rooted = work_dir.join("app.rooted.o");
        let mut objcopy = tool_command(&["llvm-objcopy", "rust-objcopy", "objcopy"])?;
        objcopy
            .arg("--globalize-symbol")
            .arg(symbol)
            .arg(&app_obj)
            .arg(&rooted);
        run_command(&mut objcopy, "objcopy globalize")?;
        rooted
    } else {
        app_obj.clone()
    };

    let mut ld = tool_command(&["ld.lld", "rust-lld", "ld"])?;
    ld.arg("-r").arg("--gc-sections").arg("-o").arg(&linked);
    if let Some(symbol) = &entry_symbol {
        ld.arg("--undefined").arg(symbol);
    }
    ld.arg(&link_app_obj);
    if !rlibs.is_empty() {
        ld.arg("--start-group");
        for rlib in &rlibs {
            ld.arg(rlib);
        }
        ld.arg("--end-group");
    }

    run_command(&mut ld, "ld")?;

    let entry_hint_hex = entry_hint_hex(&linked);

    let mut objcopy = tool_command(&["llvm-objcopy", "rust-objcopy", "objcopy"])?;
    objcopy.arg("--strip-debug").arg(&linked).arg(&stripped);
    run_command(&mut objcopy, "objcopy")?;

    let out = output_dir.join(format!("{output_name}.bp"));
    fs::create_dir_all(out.parent().ok_or("bad output path")?).map_err(io_string)?;
    write_blueprint(&out, &stripped, &entry_hint_hex)?;
    println!("packed {} -> {}", app_obj.display(), out.display());
    Ok(())
}

fn push_feature(features: &mut Vec<String>, feature: &str) {
    if !features.iter().any(|existing| existing == feature) {
        features.push(feature.to_string());
    }
}

fn run_command(cmd: &mut Command, label: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|err| format!("{label} failed to start: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {status}"))
    }
}

fn run_cargo_command(cmd: &mut Command, label: &str) -> Result<(), String> {
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

fn run_cargo_rustc_command(
    cmd: &mut Command,
    label: &str,
    deps_dir: &Path,
) -> Result<CargoBuildArtifacts, String> {
    let output = cmd
        .output()
        .map_err(|err| format!("{label} failed to start: {err}"))?;

    let mut artifacts = CargoBuildArtifacts::default();
    let mut rendered_stdout = String::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        match serde_json::from_str::<CargoJsonMessage>(line) {
            Ok(message) => match message.reason.as_str() {
                "compiler-artifact" => {
                    for filename in message.filenames {
                        let path = PathBuf::from(filename);
                        if path.extension().and_then(|ext| ext.to_str()) != Some("rlib") {
                            continue;
                        }
                        if !path.parent().is_some_and(|parent| parent == deps_dir) {
                            continue;
                        }
                        if !artifacts.rlibs.iter().any(|existing| existing == &path) {
                            artifacts.rlibs.push(path);
                        }
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
                _ => {}
            },
            Err(_) => {
                rendered_stdout.push_str(line);
                rendered_stdout.push('\n');
            }
        }
    }

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

fn write_filtered_cargo_output(
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

fn print_cargo_output_notes(label: &str, notes: &CargoOutputNotes) {
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

fn run_staged_lock_overlay_update(
    staged_manifest: &Path,
    staged_source_overlay: &[CratePatch],
    mismatch: &LockMismatch,
) -> Result<(), String> {
    let package_specs = [
        format!("{}@{}", mismatch.name, mismatch.locked_version),
        mismatch.name.clone(),
    ];

    for (index, package_spec) in package_specs.iter().enumerate() {
        let mut update = Command::new("cargo");
        update
            .arg("+nightly")
            .arg("update")
            .arg("--manifest-path")
            .arg(staged_manifest)
            .arg("-p")
            .arg(package_spec)
            .arg("--precise")
            .arg(&mismatch.overlay_version);
        push_source_overlay_configs(&mut update, staged_source_overlay);

        match update.output() {
            Ok(output) if output.status.success() => {
                let notes =
                    write_filtered_cargo_output("cargo update", &output.stdout, &output.stderr)?;
                print_cargo_output_notes("cargo update", &notes);
                return Ok(());
            }
            Ok(output) if index + 1 == package_specs.len() => {
                let notes =
                    write_filtered_cargo_output("cargo update", &output.stdout, &output.stderr)?;
                print_cargo_output_notes("cargo update", &notes);
                return Err(format!("cargo update failed with status {}", output.status));
            }
            Ok(output) => {
                let notes =
                    write_filtered_cargo_output("cargo update", &output.stdout, &output.stderr)?;
                print_cargo_output_notes("cargo update", &notes);
                println!(
                    "trueos-blueprint: retrying staged lock overlay for {} without version-qualified package id",
                    mismatch.name
                );
            }
            Err(err) => {
                return Err(format!("cargo update failed to start: {err}"));
            }
        }
    }

    Err("cargo update failed unexpectedly".to_string())
}

fn enforce_source_overlay_lock(
    staged_manifest: &Path,
    staged_source_overlay: &[CratePatch],
) -> Result<(), String> {
    if staged_source_overlay.is_empty() {
        return Ok(());
    }

    let mut generate = Command::new("cargo");
    generate
        .arg("+nightly")
        .arg("generate-lockfile")
        .arg("--manifest-path")
        .arg(staged_manifest);
    push_source_overlay_configs(&mut generate, staged_source_overlay);
    run_cargo_command(&mut generate, "cargo generate-lockfile")?;
    let lock_path = generated_lock_path(staged_manifest)?;

    let root_name = package_name(staged_manifest)?;
    let mut locked = lock_packages(&lock_path)?;
    let mut reachable = reachable_package_names(&locked, &root_name);
    let mut forced = Vec::new();
    for patch in staged_source_overlay {
        let Some(overlay_version) = package_version(&patch.path.join("Cargo.toml"))? else {
            continue;
        };
        if locked.iter().any(|package| {
            package.name == patch.name
                && package.version == overlay_version
                && package.source.is_none()
        }) {
            continue;
        };
        let Some(current) = locked.iter().find(|package| {
            package.name == patch.name && cargo_semver_same_line(&package.version, &overlay_version)
        }) else {
            continue;
        };
        if !reachable.contains(&patch.name) {
            continue;
        }

        let mismatch = LockMismatch {
            name: patch.name.clone(),
            locked_version: current.version.clone(),
            overlay_version,
        };
        run_staged_lock_overlay_update(staged_manifest, staged_source_overlay, &mismatch)?;
        forced.push(format!(
            "{} {}->{}",
            mismatch.name, mismatch.locked_version, mismatch.overlay_version
        ));
        locked = lock_packages(&lock_path)?;
        reachable = reachable_package_names(&locked, &root_name);
    }

    if !forced.is_empty() {
        println!("trueos-blueprint: source overlay lock forced: {}", forced.join(","));
    }

    let mut violations = Vec::new();
    for patch in staged_source_overlay {
        let Some(overlay_version) = package_version(&patch.path.join("Cargo.toml"))? else {
            continue;
        };
        if !reachable.contains(&patch.name) {
            continue;
        }
        for package in locked
            .iter()
            .filter(|package| package.name == patch.name && package.version == overlay_version)
        {
            if let Some(source) = &package.source {
                violations.push(format!(
                    "{} {} resolved from {} instead of overlay path {}",
                    patch.name,
                    package.version,
                    source,
                    patch.path.display()
                ));
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "source overlay lock guard failed; refusing to build with bypassed pins: {}",
            violations.join("; ")
        ))
    }
}

fn generated_lock_path(manifest_path: &Path) -> Result<PathBuf, String> {
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("bad manifest path: {}", manifest_path.display()))?;
    for ancestor in manifest_dir.ancestors() {
        let candidate = ancestor.join("Cargo.lock");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "cargo generate-lockfile did not create a Cargo.lock near {}",
        manifest_path.display()
    ))
}

fn reachable_package_names(locked: &[LockedPackage], root_name: &str) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let mut stack = vec![root_name.to_string()];

    while let Some(name) = stack.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        for package in locked.iter().filter(|package| package.name == name) {
            for dep in &package.dependencies {
                if !reachable.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
    }

    reachable
}

fn cargo_semver_same_line(version: &str, overlay: &str) -> bool {
    let Some((version_major, version_minor)) = semver_major_minor(version) else {
        return false;
    };
    let Some((overlay_major, overlay_minor)) = semver_major_minor(overlay) else {
        return false;
    };

    if overlay_major == 0 {
        version_major == 0 && version_minor == overlay_minor
    } else {
        version_major == overlay_major
    }
}

fn semver_major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn io_string(err: io::Error) -> String {
    err.to_string()
}

fn default_target_spec(app_dir: &Path) -> Result<PathBuf, String> {
    if let Some(target_spec) = env_path(TARGET_SPEC_ENV) {
        if target_spec.is_file() {
            return Ok(target_spec);
        }
        return Err(format!(
            "{TARGET_SPEC_ENV} points to missing target spec {}",
            target_spec.display()
        ));
    }

    for candidate in [
        app_dir.join("target.json"),
        app_dir.join("trueos.json"),
        app_dir.join("trueos-app.json"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    for ancestor in app_dir.ancestors().skip(1) {
        for candidate in [
            ancestor.join("target.json"),
            ancestor.join("trueos.json"),
            ancestor.join("trueos-app.json"),
        ] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "cannot infer target spec from {}; expected target.json near the blueprint Cargo.toml",
        app_dir.display()
    ))
}

fn cargo_target_spec_path(target_spec: &Path, packer_target_dir: &Path) -> Result<PathBuf, String> {
    if target_spec.file_stem().and_then(|stem| stem.to_str()) != Some("target") {
        return Ok(target_spec.to_path_buf());
    }

    let staged = packer_target_dir.join("x86_64-unknown-trueos.json");
    fs::create_dir_all(packer_target_dir).map_err(io_string)?;
    fs::copy(target_spec, &staged).map_err(io_string)?;
    Ok(staged)
}

fn cargo_cache_root(app_dir: &Path, default_packer_target_dir: &Path) -> PathBuf {
    if let Some(path) = env_path(CARGO_CACHE_DIR_ENV) {
        return path;
    }

    blueprint_root(app_dir)
        .map(|root| {
            root.join("target")
                .join("trueos-blueprint")
                .join("cargo-cache")
        })
        .unwrap_or_else(|| default_packer_target_dir.join("cargo-cache"))
}

fn blueprint_root(app_dir: &Path) -> Option<PathBuf> {
    for ancestor in app_dir.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        if manifest.is_file()
            && ancestor.join("apps.json").is_file()
            && package_name(&manifest).ok().as_deref() == Some("trueos-blueprint")
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn env_path(name: &str) -> Option<PathBuf> {
    let value = env::var_os(name)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Some(path)
    } else {
        env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

fn find_vendor_dir(app_dir: &Path, name: &str) -> Option<PathBuf> {
    for ancestor in app_dir.ancestors() {
        let candidate = ancestor.join("vendor").join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    if let Some(kernel_manifest) = trueos_kernel_manifest(app_dir)
        && let Some(kernel_root) = kernel_manifest.parent()
    {
        let candidate = kernel_root.join("vendor").join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn source_overlay_patches(
    app_dir: &Path,
    _manifest_path: &Path,
) -> Result<Vec<CratePatch>, String> {
    let mut out = Vec::new();

    if is_helix_app_dir(app_dir) {
        return Ok(out);
    }

    if let Some(kernel_manifest) = trueos_kernel_manifest(app_dir) {
        let kernel_root = kernel_manifest
            .parent()
            .ok_or_else(|| format!("bad kernel manifest path: {}", kernel_manifest.display()))?;
        for (name, path) in manifest_patch_entries(&kernel_manifest)? {
            let patch_path = resolve_manifest_path(kernel_root, &path);
            if patch_path.is_dir() {
                out.push(CratePatch {
                    name,
                    path: patch_path,
                });
            }
        }
    }

    if let Some(path) = find_vendor_dir(app_dir, "libc-0.2.186") {
        out.retain(|patch| patch.name != "libc");
        out.push(CratePatch {
            name: "libc".to_string(),
            path,
        });
    }

    if let Some(path) = find_vendor_dir(app_dir, "hyper-rustls-0.27.9") {
        out.retain(|patch| patch.name != "hyper-rustls");
        out.push(CratePatch {
            name: "hyper-rustls".to_string(),
            path,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn staged_manifest_for_overlay(
    app_dir: &Path,
    manifest_path: &Path,
    work_dir: &Path,
    build_settings: &BuildSettings,
    source_overlay: &[CratePatch],
    lock_mismatches: &[LockMismatch],
) -> Result<Option<PathBuf>, String> {
    if source_overlay.is_empty()
        && !build_settings.needs_no_std_shim
        && !build_settings.needs_entry_shim
    {
        return Ok(None);
    }

    let staged_app_dir = work_dir.join("source-overlay-app");
    copy_app_tree(app_dir, &staged_app_dir)?;
    link_kernel_sibling_for_staged_app(app_dir, work_dir)?;
    let manifest_relative = manifest_path.strip_prefix(app_dir).map_err(|_| {
        format!(
            "manifest path {} is not under app dir {}",
            manifest_path.display(),
            app_dir.display()
        )
    })?;
    let staged_manifest = staged_app_dir.join(manifest_relative);
    let nested_workspace_package = manifest_relative.components().count() > 1;
    strip_manifest_patch_section(&staged_manifest)?;
    if !nested_workspace_package {
        materialize_staged_workspace_dependencies(app_dir, work_dir, &staged_manifest)?;
    }
    materialize_hidden_build_std_pins(&staged_manifest, build_settings, source_overlay)?;
    if !nested_workspace_package {
        ensure_standalone_manifest_workspace(&staged_manifest)?;
    }
    rewrite_staged_source_for_target(app_dir, &staged_app_dir, build_settings)?;
    let staged_source_overlay = staged_source_overlay(source_overlay, work_dir);

    if lock_mismatches.is_empty() {
        return Ok(Some(staged_manifest));
    }

    println!(
        "trueos-blueprint: staged lock overlay: {}",
        lock_mismatches
            .iter()
            .map(|mismatch| format!(
                "{} {}->{}",
                mismatch.name, mismatch.locked_version, mismatch.overlay_version
            ))
            .collect::<Vec<_>>()
            .join(",")
    );

    for mismatch in lock_mismatches {
        run_staged_lock_overlay_update(&staged_manifest, &staged_source_overlay, &mismatch)?;
    }

    Ok(Some(staged_manifest))
}

fn rewrite_staged_source_for_target(
    app_dir: &Path,
    staged_app_dir: &Path,
    build_settings: &BuildSettings,
) -> Result<(), String> {
    if !build_settings.needs_no_std_shim && !build_settings.needs_entry_shim {
        return Ok(());
    }

    let relative_source = build_settings
        .source_path
        .strip_prefix(app_dir)
        .map_err(|_| {
            format!(
                "source path {} is not under app dir {}",
                build_settings.source_path.display(),
                app_dir.display()
            )
        })?;
    let staged_source = staged_app_dir.join(relative_source);
    let original = fs::read_to_string(&staged_source).map_err(io_string)?;

    let mut header = String::new();
    if build_settings.needs_no_std_shim {
        header.push_str("#![no_std]\n");
    }
    if build_settings.needs_entry_shim {
        header.push_str("#![no_main]\n");
    }

    let mut rewritten = String::with_capacity(original.len() + header.len() + 128);
    rewritten.push_str(&header);
    rewritten.push_str(&original);
    if !original.ends_with('\n') {
        rewritten.push('\n');
    }
    if build_settings.needs_entry_shim {
        if build_settings.needs_no_std_shim {
            rewritten.push_str(
                "\n#[unsafe(no_mangle)]\npub extern \"C\" fn _start() -> ! {\n    main();\n    trueos::panic_abort(\"blueprint main returned\\n\")\n}\n",
            );
        } else {
            rewritten.push_str(
                "\n#[unsafe(no_mangle)]\npub extern \"C\" fn _start() -> ! {\n    main();\n    loop {\n        core::hint::spin_loop();\n    }\n}\n",
            );
        }
    }

    fs::write(&staged_source, rewritten).map_err(io_string)
}

struct LockMismatch {
    name: String,
    locked_version: String,
    overlay_version: String,
}

struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    dependencies: Vec<String>,
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    resolve: Option<MetadataResolve>,
}

#[derive(Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    version: String,
    dependencies: Vec<MetadataDependency>,
}

#[derive(Deserialize)]
struct MetadataDependency {
    name: String,
    rename: Option<String>,
    req: String,
}

#[derive(Deserialize)]
struct MetadataResolve {
    nodes: Vec<MetadataNode>,
}

#[derive(Deserialize)]
struct MetadataNode {
    id: String,
    deps: Vec<MetadataNodeDep>,
}

#[derive(Deserialize)]
struct MetadataNodeDep {
    name: String,
    pkg: String,
}

struct VersionAlignmentTarget {
    overlay_version: String,
    parsed_overlay_version: SimpleVersion,
}

struct VersionAlignmentFinding {
    package_name: String,
    current_version: String,
    overlay_version: String,
    parent_name: String,
    parent_version: String,
    req: String,
}

struct VersionAlignmentReport {
    checked_targets: usize,
    compatible_edges: usize,
    unresolved_edges: usize,
    unparsed_requirements: usize,
    incompatible: Vec<VersionAlignmentFinding>,
}

fn preflight_source_overlay_version_alignment(
    app_dir: &Path,
    manifest_path: &Path,
    lock_mismatches: &[LockMismatch],
) -> Result<(), String> {
    if lock_mismatches.is_empty() {
        return Ok(());
    }

    let report = match source_overlay_version_alignment(app_dir, manifest_path, lock_mismatches) {
        Ok(report) => report,
        Err(err) => {
            println!("trueos-blueprint: version alignment skipped: {err}");
            return Ok(());
        }
    };

    if report.incompatible.is_empty() {
        println!(
            "trueos-blueprint: version alignment: checked {} overlay change(s), {} active edge(s) accept the forced version",
            report.checked_targets, report.compatible_edges
        );
        if report.unresolved_edges > 0 || report.unparsed_requirements > 0 {
            println!(
                "trueos-blueprint: version alignment notes: {} unresolved edge(s), {} unparsed requirement(s)",
                report.unresolved_edges, report.unparsed_requirements
            );
        }
        return Ok(());
    }

    Err(format!(
        "version alignment failed before staged lock overlay:\n{}\ntrueos-blueprint: at least one forced overlay version falls outside a depender's declared range; API compatibility was not attempted",
        report
            .incompatible
            .iter()
            .map(|finding| format!(
                "  {} {} -> {} blocked by {} {} requiring {}",
                finding.package_name,
                finding.current_version,
                finding.overlay_version,
                finding.parent_name,
                finding.parent_version,
                finding.req
            ))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn source_overlay_version_alignment(
    app_dir: &Path,
    manifest_path: &Path,
    lock_mismatches: &[LockMismatch],
) -> Result<VersionAlignmentReport, String> {
    let metadata = cargo_metadata(app_dir, manifest_path)?;
    let resolve = metadata
        .resolve
        .ok_or_else(|| "cargo metadata returned no resolve graph".to_string())?;

    let mut packages_by_id = HashMap::new();
    for package in &metadata.packages {
        packages_by_id.insert(package.id.clone(), package);
    }

    let mut overlay_targets = BTreeMap::new();
    for mismatch in lock_mismatches {
        match overlay_targets.entry(mismatch.name.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                let parsed_overlay_version = SimpleVersion::parse(&mismatch.overlay_version)
                    .map_err(|err| {
                        format!(
                            "failed to parse overlay version {} for {}: {err}",
                            mismatch.overlay_version, mismatch.name
                        )
                    })?;
                slot.insert(VersionAlignmentTarget {
                    overlay_version: mismatch.overlay_version.clone(),
                    parsed_overlay_version,
                });
            }
            std::collections::btree_map::Entry::Occupied(existing)
                if existing.get().overlay_version != mismatch.overlay_version =>
            {
                return Err(format!(
                    "overlay version for {} is inconsistent: {} vs {}",
                    mismatch.name,
                    existing.get().overlay_version,
                    mismatch.overlay_version
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }

    let mut compatible_edges = 0usize;
    let mut unresolved_edges = 0usize;
    let mut unparsed_requirements = 0usize;
    let mut incompatible = Vec::new();

    for node in resolve.nodes {
        let Some(parent_package) = packages_by_id.get(&node.id) else {
            continue;
        };

        for dep in node.deps {
            let Some(dep_package) = packages_by_id.get(&dep.pkg) else {
                continue;
            };
            let Some(target) = overlay_targets.get(&dep_package.name) else {
                continue;
            };
            if dep_package.version == target.overlay_version {
                continue;
            }

            let Some(declared_dependency) = parent_package.dependencies.iter().find(|candidate| {
                dependency_display_name(candidate) == dep.name || candidate.name == dep_package.name
            }) else {
                unresolved_edges += 1;
                continue;
            };

            let req_matches =
                match version_req_matches(&declared_dependency.req, &target.parsed_overlay_version)
                {
                    Ok(matches) => matches,
                    Err(_) => {
                        unparsed_requirements += 1;
                        continue;
                    }
                };

            if req_matches {
                compatible_edges += 1;
                continue;
            }

            incompatible.push(VersionAlignmentFinding {
                package_name: dep_package.name.clone(),
                current_version: dep_package.version.clone(),
                overlay_version: target.overlay_version.clone(),
                parent_name: parent_package.name.clone(),
                parent_version: parent_package.version.clone(),
                req: declared_dependency.req.clone(),
            });
        }
    }

    incompatible.sort_by(|a, b| {
        a.package_name
            .cmp(&b.package_name)
            .then(a.parent_name.cmp(&b.parent_name))
            .then(a.parent_version.cmp(&b.parent_version))
    });
    incompatible.dedup_by(|left, right| {
        left.package_name == right.package_name
            && left.current_version == right.current_version
            && left.overlay_version == right.overlay_version
            && left.parent_name == right.parent_name
            && left.parent_version == right.parent_version
            && left.req == right.req
    });

    Ok(VersionAlignmentReport {
        checked_targets: overlay_targets.len(),
        compatible_edges,
        unresolved_edges,
        unparsed_requirements,
        incompatible,
    })
}

fn cargo_metadata(app_dir: &Path, manifest_path: &Path) -> Result<CargoMetadata, String> {
    let mut metadata = Command::new("cargo");
    if let Some(manifest_dir) = manifest_path.parent() {
        metadata.current_dir(manifest_dir);
    } else {
        metadata.current_dir(app_dir);
    }
    metadata
        .arg("+nightly")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--locked")
        .arg("--manifest-path")
        .arg(manifest_path);
    let output = metadata
        .output()
        .map_err(|err| format!("cargo metadata failed to start: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(format!("cargo metadata failed with status {}", output.status));
        }
        return Err(format!("cargo metadata failed: {stderr}"));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse cargo metadata JSON: {err}"))
}

fn dependency_display_name(dependency: &MetadataDependency) -> &str {
    dependency.rename.as_deref().unwrap_or(&dependency.name)
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct SimpleVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl SimpleVersion {
    fn parse(value: &str) -> Result<Self, String> {
        let core = value
            .split_once('+')
            .map(|(prefix, _)| prefix)
            .unwrap_or(value)
            .split_once('-')
            .map(|(prefix, _)| prefix)
            .unwrap_or(value);
        let mut parts = core.split('.');
        let major = parse_u64_component(parts.next(), value)?;
        let minor = parse_u64_component(parts.next(), value)?;
        let patch = parse_u64_component(parts.next(), value)?;
        if parts.next().is_some() {
            return Err(format!("unsupported version `{value}`"));
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

struct ReqVersion {
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
}

fn version_req_matches(req: &str, version: &SimpleVersion) -> Result<bool, String> {
    if req.contains("||") {
        return Err(format!("unsupported disjunctive requirement `{req}`"));
    }

    for raw_token in req.split(',') {
        let token = raw_token.trim();
        if token.is_empty() || token == "*" {
            continue;
        }
        if !requirement_token_matches(token, version)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn requirement_token_matches(token: &str, version: &SimpleVersion) -> Result<bool, String> {
    if let Some(rest) = token.strip_prefix(">=") {
        return Ok(version >= &req_lower_bound(&parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix("<=") {
        return Ok(version <= &req_lower_bound(&parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix('>') {
        return Ok(version > &req_lower_bound(&parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix('<') {
        return Ok(version < &req_lower_bound(&parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix('=') {
        return Ok(exact_req_matches(version, &parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix('^') {
        return Ok(caret_req_matches(version, &parse_req_version(rest.trim(), token)?));
    }
    if let Some(rest) = token.strip_prefix('~') {
        return Ok(tilde_req_matches(version, &parse_req_version(rest.trim(), token)?));
    }
    if token.contains('*') || token.contains('x') || token.contains('X') {
        return Ok(wildcard_req_matches(version, &parse_req_prefix(token)?));
    }
    Ok(caret_req_matches(version, &parse_req_version(token, token)?))
}

fn exact_req_matches(version: &SimpleVersion, req: &ReqVersion) -> bool {
    version.major == req.major
        && req.minor.is_none_or(|minor| version.minor == minor)
        && req.patch.is_none_or(|patch| version.patch == patch)
}

fn wildcard_req_matches(version: &SimpleVersion, req: &ReqVersion) -> bool {
    version.major == req.major
        && req.minor.is_none_or(|minor| version.minor == minor)
        && req.patch.is_none_or(|patch| version.patch == patch)
}

fn caret_req_matches(version: &SimpleVersion, req: &ReqVersion) -> bool {
    let lower = req_lower_bound(req);
    version >= &lower && version < &caret_upper_bound(req)
}

fn tilde_req_matches(version: &SimpleVersion, req: &ReqVersion) -> bool {
    let lower = req_lower_bound(req);
    version >= &lower && version < &tilde_upper_bound(req)
}

fn req_lower_bound(req: &ReqVersion) -> SimpleVersion {
    SimpleVersion {
        major: req.major,
        minor: req.minor.unwrap_or(0),
        patch: req.patch.unwrap_or(0),
    }
}

fn caret_upper_bound(req: &ReqVersion) -> SimpleVersion {
    if req.major > 0 {
        return SimpleVersion {
            major: req.major + 1,
            minor: 0,
            patch: 0,
        };
    }

    let Some(minor) = req.minor else {
        return SimpleVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
    };

    if minor > 0 {
        return SimpleVersion {
            major: 0,
            minor: minor + 1,
            patch: 0,
        };
    }

    let Some(patch) = req.patch else {
        return SimpleVersion {
            major: 0,
            minor: 1,
            patch: 0,
        };
    };

    SimpleVersion {
        major: 0,
        minor: 0,
        patch: patch + 1,
    }
}

fn tilde_upper_bound(req: &ReqVersion) -> SimpleVersion {
    match req.minor {
        Some(minor) => SimpleVersion {
            major: req.major,
            minor: minor + 1,
            patch: 0,
        },
        None => SimpleVersion {
            major: req.major + 1,
            minor: 0,
            patch: 0,
        },
    }
}

fn parse_req_version(value: &str, original: &str) -> Result<ReqVersion, String> {
    let core = value
        .split_once('+')
        .map(|(prefix, _)| prefix)
        .unwrap_or(value)
        .split_once('-')
        .map(|(prefix, _)| prefix)
        .unwrap_or(value)
        .trim();
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return Err(format!("unsupported requirement `{original}`"));
    }
    let major = parse_req_component(parts.first().copied(), original)?;
    let minor = parse_optional_req_component(parts.get(1).copied(), original)?;
    let patch = parse_optional_req_component(parts.get(2).copied(), original)?;
    Ok(ReqVersion {
        major,
        minor,
        patch,
    })
}

fn parse_req_prefix(token: &str) -> Result<ReqVersion, String> {
    let mut major = None;
    let mut minor = None;
    let mut patch = None;
    for (index, part) in token.split('.').enumerate() {
        if part == "*" || part == "x" || part == "X" {
            break;
        }
        match index {
            0 => major = Some(parse_req_component(Some(part), token)?),
            1 => minor = Some(parse_req_component(Some(part), token)?),
            2 => patch = Some(parse_req_component(Some(part), token)?),
            _ => return Err(format!("unsupported requirement `{token}`")),
        }
    }
    Ok(ReqVersion {
        major: major.ok_or_else(|| format!("unsupported requirement `{token}`"))?,
        minor,
        patch,
    })
}

fn parse_u64_component(component: Option<&str>, original: &str) -> Result<u64, String> {
    let value = component.ok_or_else(|| format!("unsupported version `{original}`"))?;
    value
        .parse::<u64>()
        .map_err(|_| format!("unsupported version `{original}`"))
}

fn parse_req_component(component: Option<&str>, original: &str) -> Result<u64, String> {
    let value = component.ok_or_else(|| format!("unsupported requirement `{original}`"))?;
    value
        .parse::<u64>()
        .map_err(|_| format!("unsupported requirement `{original}`"))
}

fn parse_optional_req_component(
    component: Option<&str>,
    original: &str,
) -> Result<Option<u64>, String> {
    match component {
        Some("*") | Some("x") | Some("X") => Ok(None),
        Some(value) => value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| format!("unsupported requirement `{original}`")),
        None => Ok(None),
    }
}

fn source_overlay_lock_mismatches(
    app_dir: &Path,
    source_overlay: &[CratePatch],
) -> Result<Vec<LockMismatch>, String> {
    let lock_path = app_dir.join("Cargo.lock");
    if !lock_path.is_file() {
        return Ok(Vec::new());
    }

    let lock_packages = lock_package_versions(&lock_path)?;
    let mut out = Vec::new();
    for patch in source_overlay {
        let Some(overlay_version) = package_version(&patch.path.join("Cargo.toml"))? else {
            continue;
        };
        let has_matching_locked_version = lock_packages.iter().any(|(name, locked_version)| {
            name == &patch.name && locked_version == &overlay_version
        });
        if has_matching_locked_version {
            continue;
        }
        for (name, locked_version) in &lock_packages {
            if name == &patch.name && locked_version != &overlay_version {
                if !cargo_semver_same_line(locked_version, &overlay_version) {
                    continue;
                }
                out.push(LockMismatch {
                    name: name.clone(),
                    locked_version: locked_version.clone(),
                    overlay_version: overlay_version.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn lock_package_versions(lock_path: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(lock_packages(lock_path)?
        .into_iter()
        .map(|package| (package.name, package.version))
        .collect())
}

fn lock_packages(lock_path: &Path) -> Result<Vec<LockedPackage>, String> {
    let cargo_lock = fs::read_to_string(lock_path).map_err(io_string)?;
    let mut out = Vec::new();
    let mut in_package = false;
    let mut in_dependencies = false;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut source: Option<String> = None;
    let mut dependencies = Vec::new();

    for line in cargo_lock.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if in_package && let (Some(name), Some(version)) = (name.take(), version.take()) {
                out.push(LockedPackage {
                    name,
                    version,
                    source: source.take(),
                    dependencies,
                });
            }
            in_package = true;
            in_dependencies = false;
            name = None;
            version = None;
            source = None;
            dependencies = Vec::new();
            continue;
        }
        if !in_package {
            continue;
        }
        if in_dependencies {
            if trimmed == "]" {
                in_dependencies = false;
                continue;
            }
            if let Some(dep) = lock_dependency_name(trimmed) {
                dependencies.push(dep);
            }
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            match key.trim() {
                "name" => name = toml_string_value(value.trim()),
                "version" => version = toml_string_value(value.trim()),
                "source" => source = toml_string_value(value.trim()),
                "dependencies" if value.trim() == "[" => in_dependencies = true,
                _ => {}
            }
        }
    }

    if in_package && let (Some(name), Some(version)) = (name, version) {
        out.push(LockedPackage {
            name,
            version,
            source,
            dependencies,
        });
    }
    Ok(out)
}

fn lock_dependency_name(line: &str) -> Option<String> {
    let value = line.trim().trim_end_matches(',');
    let dep = toml_string_value(value)?;
    let name = dep.split_whitespace().next()?;
    Some(name.to_string())
}

fn package_version(manifest_path: &Path) -> Result<Option<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "version"
        {
            return Ok(toml_string_value(value.trim()));
        }
    }
    Ok(None)
}

fn copy_app_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(io_string)?;
    for entry in fs::read_dir(from).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if matches!(name_str, ".git" | "target" | "dist") {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        let file_type = entry.file_type().map_err(io_string)?;
        if file_type.is_dir() {
            copy_app_tree(&src, &dst)?;
        } else if file_type.is_file() {
            fs::copy(&src, &dst).map_err(io_string)?;
        } else if file_type.is_symlink()
            && let Ok(target) = fs::read_link(&src)
        {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, dst).map_err(io_string)?;
        }
    }
    Ok(())
}

fn link_kernel_sibling_for_staged_app(app_dir: &Path, work_dir: &Path) -> Result<(), String> {
    let Some(kernel_manifest) = trueos_kernel_manifest(app_dir) else {
        return Ok(());
    };
    let Some(kernel_root) = kernel_manifest.parent() else {
        return Ok(());
    };
    let staging_root = work_dir.parent().unwrap_or(work_dir);
    let app_target_root = app_dir.join("target");

    for ancestor in app_dir.ancestors() {
        let blueprint_api = ancestor.join("api");
        if blueprint_api.join("Cargo.toml").is_file() {
            link_staged_sibling(&staging_root.join("api"), &blueprint_api)?;
            break;
        }
    }

    link_staged_sibling(&app_target_root.join("crates"), &kernel_root.join("crates"))?;
    link_staged_sibling(&staging_root.join("TRUEOS"), kernel_root)?;
    link_staged_sibling(&staging_root.join("vendor"), &kernel_root.join("vendor"))?;
    link_staged_sibling(&staging_root.join("crates"), &kernel_root.join("crates"))?;

    Ok(())
}

fn link_staged_sibling(link: &Path, target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }

    if let Ok(metadata) = fs::symlink_metadata(link) {
        let file_type = metadata.file_type();
        if file_type.is_symlink()
            && let Ok(existing_target) = fs::read_link(link)
            && existing_target == target
        {
            return Ok(());
        }

        if file_type.is_dir() && !file_type.is_symlink() {
            fs::remove_dir_all(link).map_err(io_string)?;
        } else {
            fs::remove_file(link).map_err(io_string)?;
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(io_string)?;
    }
    Ok(())
}

fn push_source_overlay_configs(cmd: &mut Command, source_overlay: &[CratePatch]) {
    for patch in source_overlay {
        cmd.arg("--config").arg(format!(
            "patch.crates-io.{}.path={}",
            patch.name,
            toml_string(&patch.path.to_string_lossy())
        ));
    }
}

fn staged_source_overlay(source_overlay: &[CratePatch], _work_dir: &Path) -> Vec<CratePatch> {
    source_overlay
        .iter()
        .map(|patch| CratePatch {
            name: patch.name.clone(),
            path: patch.path.clone(),
        })
        .collect()
}

fn trueos_kernel_manifest(app_dir: &Path) -> Option<PathBuf> {
    if let Some(root) = env::var_os("TRUEOS_BLUEPRINT_KERNEL_ROOT") {
        let candidate = PathBuf::from(root).join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    for ancestor in app_dir.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if candidate.is_file()
            && ancestor.join("vendor").is_dir()
            && package_name(&candidate).ok().as_deref() == Some("TRUEOS")
        {
            return Some(candidate);
        }

        let sibling = ancestor.join("TRUEOS").join("Cargo.toml");
        if sibling.is_file()
            && sibling
                .parent()
                .is_some_and(|root| root.join("vendor").is_dir())
        {
            return Some(sibling);
        }
    }

    None
}

fn manifest_patch_entries(manifest_path: &Path) -> Result<Vec<(String, String)>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_patch = false;
    let mut out = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_patch = trimmed == "[patch.crates-io]";
            continue;
        }
        if !in_patch || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let Some(path) = inline_table_path(value.trim()) else {
            continue;
        };
        out.push((name.trim().trim_matches('"').to_string(), path));
    }
    Ok(out)
}

fn strip_manifest_patch_section(manifest_path: &Path) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut out = String::with_capacity(cargo_toml.len());
    let mut in_patch = false;

    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_patch = trimmed == "[patch.crates-io]";
            if in_patch {
                continue;
            }
        }
        if in_patch {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    fs::write(manifest_path, out).map_err(io_string)
}

fn materialize_staged_workspace_dependencies(
    app_dir: &Path,
    work_dir: &Path,
    manifest_path: &Path,
) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let blueprint_root = blueprint_root(app_dir).unwrap_or_else(|| app_dir.to_path_buf());
    let mut changed = false;
    let mut out = String::with_capacity(cargo_toml.len());

    for line in cargo_toml.lines() {
        if let Some(dep_name) = workspace_dependency_name(line) {
            out.push_str(&materialized_workspace_dependency(&blueprint_root, work_dir, &dep_name)?);
            out.push('\n');
            changed = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if changed {
        fs::write(manifest_path, out).map_err(io_string)?;
    }
    Ok(())
}

fn materialize_hidden_build_std_pins(
    manifest_path: &Path,
    build_settings: &BuildSettings,
    source_overlay: &[CratePatch],
) -> Result<(), String> {
    if !matches!(build_settings.flavor, BuildFlavor::TokioStd) {
        return Ok(());
    }
    if manifest_has_dependency(manifest_path, "libc")? {
        return Ok(());
    }
    let Some(libc_patch) = source_overlay.iter().find(|patch| patch.name == "libc") else {
        return Ok(());
    };
    let Some(libc_version) = package_version(&libc_patch.path.join("Cargo.toml"))? else {
        return Ok(());
    };

    let mut cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    if !cargo_toml.ends_with('\n') {
        cargo_toml.push('\n');
    }
    cargo_toml.push_str(&format!(
        "\n# Pinned because Rust build-std pulls libc outside the app dependency graph.\nlibc = {{ version = \"={libc_version}\", default-features = false }}\n"
    ));
    fs::write(manifest_path, cargo_toml).map_err(io_string)
}

fn workspace_dependency_name(line: &str) -> Option<String> {
    let line = line.split('#').next().unwrap_or("").trim();
    let Some((key, value)) = line.split_once('=') else {
        return None;
    };
    let key = key.trim();
    let value = value.trim();
    if value == "true"
        && let Some(dep_name) = key.strip_suffix(".workspace")
    {
        return Some(dep_name.to_string());
    }
    if value.contains("workspace") && value.contains("true") {
        return Some(key.to_string());
    }
    None
}

fn materialized_workspace_dependency(
    blueprint_root: &Path,
    work_dir: &Path,
    dep_name: &str,
) -> Result<String, String> {
    let line = match dep_name {
        "anyhow" => "anyhow = { version = \"1.0\", default-features = false }".to_string(),
        "axum" => {
            "axum = { version = \"0.8.9\", default-features = false, features = [\"http1\", \"json\", \"tokio\"] }"
                .to_string()
        }
        "colored" => "colored = \"2.1\"".to_string(),
        "glob" => "glob = \"0.3\"".to_string(),
        "http-body-util" => {
            path_dependency_line(dep_name, &blueprint_root.join("../../vendor/http-body-util-0.1.3"))
        }
        "hyper" => {
            "hyper = { version = \"1.9\", default-features = false, features = [\"client\", \"server\", \"http1\"] }"
                .to_string()
        }
        "hyper-util" => {
            format!(
                "hyper-util = {{ path = {}, default-features = false, features = [\"tokio\"] }}",
                toml_string(&blueprint_root.join("../../vendor/hyper-util-0.1.20").display().to_string())
            )
        }
        "ignore" => "ignore = \"0.4\"".to_string(),
        "libm" => "libm = { version = \"0.2\", default-features = false }".to_string(),
        "regex" => {
            "regex = { version = \"1\", default-features = false, features = [\"perf\"] }"
                .to_string()
        }
        "reqwest" => {
            "reqwest = { version = \"0.13.3\", default-features = false, features = [\"json\"] }"
                .to_string()
        }
        "rustls" => {
            "rustls = { version = \"0.23.27\", default-features = false, features = [\"std\", \"tls12\"] }"
                .to_string()
        }
        "rustls-rustcrypto" => {
            format!(
                "rustls-rustcrypto = {{ path = {}, default-features = false, features = [\"std\", \"tls12\"] }}",
                toml_string(&blueprint_root.join("../../vendor/rustls-rustcrypto-0.0.2-alpha").display().to_string())
            )
        }
        "rustyline" => "rustyline = \"14.0\"".to_string(),
        "serde" => {
            "serde = { version = \"1.0\", default-features = false, features = [\"derive\", \"alloc\"] }"
                .to_string()
        }
        "serde_json" => {
            "serde_json = { version = \"1.0\", default-features = false, features = [\"alloc\"] }"
                .to_string()
        }
        "serde_yaml" => "serde_yaml = \"0.9\"".to_string(),
        "tempfile" => "tempfile = \"3\"".to_string(),
        "tokio" => {
            "tokio = { version = \"1.52.3\", default-features = false, features = [\"full\"] }"
                .to_string()
        }
        "tokio-rustls" => {
            format!(
                "tokio-rustls = {{ path = {}, default-features = false, features = [\"tls12\"] }}",
                toml_string(&blueprint_root.join("../../vendor/tokio-rustls-0.26.4").display().to_string())
            )
        }
        "tower" => {
            "tower = { version = \"0.5\", default-features = false, features = [\"util\"] }"
                .to_string()
        }
        "trueos" => path_dependency_line(dep_name, &blueprint_root.join("api")),
        "trueos-chat" => path_dependency_line(dep_name, &blueprint_root.join("apps/chatserver/trueos-chat")),
        "trueos-currency" => {
            path_dependency_line(dep_name, &blueprint_root.join("apps/currency_reqwest/trueos-currency"))
        }
        "trueos-flags" => {
            path_dependency_line(dep_name, &blueprint_root.join("apps/flags/trueos-flags"))
        }
        "trueos-gfx-core" => format!(
            "trueos-gfx-core = {{ path = {}, features = [\"alloc\"] }}",
            toml_string(&blueprint_root.join("../trueos-gfx-core").display().to_string())
        ),
        "trueos-tetris" => path_dependency_line(
            dep_name,
            &stage_trueos_tetris_crate(blueprint_root, work_dir)?,
        ),
        "trueos-weather" => {
            path_dependency_line(dep_name, &blueprint_root.join("apps/weather/trueos-weather"))
        }
        "webpki-roots" => {
            "webpki-roots = { version = \"1\", default-features = false }".to_string()
        }
        other => return Err(format!("unsupported workspace dependency `{other}` in {}", blueprint_root.display())),
    };
    Ok(line)
}

fn path_dependency_line(dep_name: &str, path: &Path) -> String {
    format!("{dep_name} = {{ path = {} }}", toml_string(&path.display().to_string()))
}

fn stage_trueos_tetris_crate(blueprint_root: &Path, work_dir: &Path) -> Result<PathBuf, String> {
    let source = blueprint_root.join("apps/crates/trueos-tetris");
    let staged = work_dir.join("blueprint-crates").join("trueos-tetris");
    let trueos_v = fs::canonicalize(blueprint_root.join("../trueos-v")).map_err(io_string)?;
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    rewrite_manifest_dependency_path(
        &staged.join("Cargo.toml"),
        "v",
        &trueos_v.display().to_string(),
    )?;
    Ok(staged)
}

fn rewrite_manifest_dependency_path(
    manifest_path: &Path,
    dep_name: &str,
    path: &str,
) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut out = String::with_capacity(cargo_toml.len());
    let mut changed = false;

    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        let is_dep = trimmed
            .split_once('=')
            .map(|(key, _)| key.trim() == dep_name)
            .unwrap_or(false);
        if is_dep {
            out.push_str(dep_name);
            out.push_str(" = { path = ");
            out.push_str(&toml_string(path));
            out.push_str(" }\n");
            changed = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !changed {
        return Err(format!("missing dependency `{dep_name}` in {}", manifest_path.display()));
    }

    fs::write(manifest_path, out).map_err(io_string)
}

fn ensure_standalone_manifest_workspace(manifest_path: &Path) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    if cargo_toml
        .lines()
        .any(|line| line.split('#').next().unwrap_or("").trim() == "[workspace]")
    {
        return Ok(());
    }

    let mut out = cargo_toml;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n[workspace]\n");

    fs::write(manifest_path, out).map_err(io_string)
}

fn push_extra_rustflags(command: &mut Command, flags: &[&str]) {
    let mut encoded = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    for flag in flags {
        if !encoded.is_empty() {
            encoded.push(RUSTFLAGS_ENCODED_SEPARATOR);
        }
        encoded.push_str(flag);
    }
    command.env("CARGO_ENCODED_RUSTFLAGS", encoded);
}

fn push_bindgen_clang_args(command: &mut Command) {
    let Some(builtin_include) = bindgen_builtin_include_dir() else {
        return;
    };
    let mut args = env::var("BINDGEN_EXTRA_CLANG_ARGS").unwrap_or_default();
    push_clang_isystem_arg(&mut args, &builtin_include);
    if let Some(include_dir) = host_multiarch_include_dir() {
        push_clang_isystem_arg(&mut args, &include_dir);
    }
    command.env("BINDGEN_EXTRA_CLANG_ARGS", args);
}

fn push_trueos_cc_flags(command: &mut Command) {
    push_env_words(
        command,
        "CFLAGS",
        &["-DROCKSDB_PLATFORM_POSIX", "-DROCKSDB_LIB_IO_POSIX", "-DOS_LINUX"],
    );
    push_env_words(
        command,
        "CXXFLAGS",
        &["-DROCKSDB_PLATFORM_POSIX", "-DROCKSDB_LIB_IO_POSIX", "-DOS_LINUX"],
    );
}

fn push_env_words(command: &mut Command, key: &str, words: &[&str]) {
    let mut value = env::var(key).unwrap_or_default();
    for word in words {
        if !value.split_whitespace().any(|existing| existing == *word) {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(word);
        }
    }
    command.env(key, value);
}

fn push_clang_isystem_arg(args: &mut String, include_dir: &Path) {
    if !args.is_empty() {
        args.push(' ');
    }
    args.push_str("-isystem ");
    args.push_str(&include_dir.to_string_lossy());
}

fn bindgen_builtin_include_dir() -> Option<PathBuf> {
    if let Ok(output) = Command::new("clang").arg("-print-resource-dir").output()
        && output.status.success()
    {
        let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !resource_dir.is_empty() {
            let builtin_include = PathBuf::from(resource_dir).join("include");
            if builtin_include.is_dir() {
                return Some(builtin_include);
            }
        }
    }

    let Ok(output) = Command::new("cc").arg("-print-file-name=include").output() else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let builtin_include = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    builtin_include.is_dir().then_some(builtin_include)
}

fn host_multiarch_include_dir() -> Option<PathBuf> {
    let Ok(output) = Command::new("cc").arg("-dumpmachine").output() else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let triple = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if triple.is_empty() {
        return None;
    }
    let include_dir = PathBuf::from("/usr/include").join(triple);
    include_dir.is_dir().then_some(include_dir)
}

fn inline_table_path(value: &str) -> Option<String> {
    let table = value
        .trim()
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))?;
    for item in table.split(',') {
        let (key, value) = item.split_once('=')?;
        if key.trim() == "path" {
            return toml_string_value(value.trim());
        }
    }
    None
}

fn toml_string_value(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            escaped => out.push(escaped),
        }
    }
    Some(out)
}

fn resolve_manifest_path(manifest_root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_root.join(path)
    }
}

fn toml_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn package_name(manifest_path: &Path) -> Result<String, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("name") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err(format!("failed to read package name from {}", manifest_path.display()))
}

fn package_bin_name(manifest_path: &Path) -> Result<Option<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_package = false;
    let mut in_bin = false;
    let mut default_run = None;
    let mut first_bin = None;

    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            in_bin = trimmed == "[[bin]]";
            continue;
        }

        if in_package && trimmed.starts_with("default-run") {
            if let Some((_, value)) = trimmed.split_once('=') {
                default_run = toml_string_value(value.trim());
            }
            continue;
        }

        if in_bin && first_bin.is_none() && trimmed.starts_with("name") {
            if let Some((_, value)) = trimmed.split_once('=') {
                first_bin = toml_string_value(value.trim());
            }
        }
    }

    Ok(default_run.or(first_bin))
}

fn package_app_specs(app_dir: &Path) -> Result<Vec<PackageAppSpec>, String> {
    let mut specs = Vec::new();
    for app_name in registered_app_names(app_dir)? {
        specs.push(package_app_spec_required(app_dir, &app_name)?);
    }
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

fn package_app_spec(app_dir: &Path, app_name: &str) -> Result<Option<PackageAppSpec>, String> {
    if !registered_app_names(app_dir)?
        .iter()
        .any(|name| name == app_name)
    {
        return Ok(None);
    }
    package_app_spec_required(app_dir, app_name).map(Some)
}

fn package_app_spec_required(app_dir: &Path, app_name: &str) -> Result<PackageAppSpec, String> {
    let dir = app_dir.join("apps").join(app_name);
    let mut manifest_path = dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("registered app `{app_name}` is missing {}", manifest_path.display()));
    }

    let mut name = match package_name(&manifest_path) {
        Ok(name) => name,
        Err(_) => {
            let package_manifest_path = virtual_package_app_manifest_path(&dir, app_name);
            if !package_manifest_path.is_file() {
                return Err(format!(
                    "registered app `{app_name}` has virtual manifest {} without a known package manifest",
                    manifest_path.display()
                ));
            }
            manifest_path = package_manifest_path;
            package_name(&manifest_path)?
        }
    };
    if name != app_name && !virtual_package_app_alias(app_name) {
        return Err(format!(
            "registered app `{app_name}` has package name `{name}` in {}",
            manifest_path.display()
        ));
    }
    if virtual_package_app_alias(app_name) {
        name = app_name.to_string();
    }

    Ok(PackageAppSpec {
        name,
        dir,
        manifest_path,
    })
}

fn virtual_package_app_manifest_path(dir: &Path, app_name: &str) -> PathBuf {
    match app_name {
        "helix" => dir.join("helix-term").join("Cargo.toml"),
        "matrix" => dir.join("src").join("main").join("Cargo.toml"),
        _ => dir.join("src").join("main").join("Cargo.toml"),
    }
}

fn virtual_package_app_alias(app_name: &str) -> bool {
    matches!(app_name, "helix" | "matrix")
}

fn registered_app_names(app_dir: &Path) -> Result<Vec<String>, String> {
    let registry_path = app_dir.join("apps.json");
    let raw = fs::read_to_string(&registry_path)
        .map_err(|err| format!("failed to read {}: {err}", registry_path.display()))?;
    let registry: AppRegistry = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", registry_path.display()))?;

    let mut out = Vec::with_capacity(registry.apps.len());
    for name in registry.apps {
        if name.trim().is_empty() {
            return Err(format!("empty app name in {}", registry_path.display()));
        }
        if out.iter().any(|existing| existing == &name) {
            return Err(format!(
                "duplicate registered app `{name}` in {}",
                registry_path.display()
            ));
        }
        out.push(name);
    }
    Ok(out)
}

fn package_blueprint_profile(manifest_path: &Path) -> Result<Option<CargoProfile>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_metadata = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_metadata = trimmed == "[package.metadata.trueos-blueprint]";
            continue;
        }
        if !in_metadata {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "profile" {
            continue;
        }
        return match toml_string_value(value.trim()).as_deref() {
            Some("dev") | Some("debug") => Ok(Some(CargoProfile::Dev)),
            Some("release") => Ok(Some(CargoProfile::Release)),
            Some(other) => Err(format!(
                "unsupported trueos-blueprint profile `{other}` in {}",
                manifest_path.display()
            )),
            None => Err(format!("bad trueos-blueprint profile in {}", manifest_path.display())),
        };
    }
    Ok(None)
}

fn manifest_declared_features(manifest_path: &Path) -> Result<Vec<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_features = false;
    let mut out = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features || trimmed.is_empty() {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        out.push(key.trim().to_string());
    }
    Ok(out)
}

fn manifest_has_dependency(manifest_path: &Path, dependency_name: &str) -> Result<bool, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_dependencies = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_dependencies =
                matches!(trimmed, "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]");
            continue;
        }
        if !in_dependencies || trimmed.is_empty() {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() == dependency_name || key.trim().starts_with(&format!("{dependency_name}.")) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn push_app_or_trueos_feature(
    features: &mut Vec<String>,
    feature: &str,
    declared_features: &[String],
    has_trueos_dependency: bool,
) {
    if declared_features.iter().any(|declared| declared == feature) {
        push_feature(features, feature);
    } else if has_trueos_dependency {
        push_feature(features, &format!("trueos/{feature}"));
    }
}

fn example_required_features(
    manifest_path: &Path,
    example_name: &str,
) -> Result<Vec<String>, String> {
    example_specs(manifest_path)?
        .into_iter()
        .find(|example| example.name == example_name)
        .map(|example| example.required_features)
        .ok_or_else(|| format!("unknown example `{example_name}`"))
}

fn example_specs(manifest_path: &Path) -> Result<Vec<ExampleSpec>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut specs = Vec::new();
    let mut in_example = false;
    let mut current_name: Option<String> = None;
    let mut current_required_features = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_example && let Some(name) = current_name.take() {
                specs.push(ExampleSpec {
                    name,
                    required_features: core::mem::take(&mut current_required_features),
                });
            }
            in_example = trimmed == "[[example]]";
            if in_example {
                current_name = None;
                current_required_features.clear();
            }
            continue;
        }
        if !in_example {
            continue;
        }
        if trimmed.starts_with("name") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            current_name = Some(value.trim().trim_matches('"').to_string());
        } else if trimmed.starts_with("required-features") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            current_required_features = parse_string_array(value.trim());
        }
    }
    if in_example && let Some(name) = current_name {
        specs.push(ExampleSpec {
            name,
            required_features: current_required_features,
        });
    }
    Ok(specs)
}

fn parse_string_array(value: &str) -> Vec<String> {
    let Some(inner) = value
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.trim_matches('"').to_string())
        .collect()
}

fn workdir(app_dir: &Path, packer_target_dir: &Path, output_name: &str) -> Result<PathBuf, String> {
    let safe_name = sanitize_path_component(output_name);
    if is_helix_app_dir(app_dir) {
        return Ok(env::temp_dir()
            .join("trueos-blueprint-work")
            .join(safe_name));
    }

    Ok(packer_target_dir.join("work").join(safe_name))
}

fn is_helix_app_dir(app_dir: &Path) -> bool {
    app_dir.file_name().and_then(|name| name.to_str()) == Some("helix")
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn reset_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(io_string)?;
    }
    fs::create_dir_all(path).map_err(io_string)
}
