use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

mod build_plan;

use build_plan::{BuildFlavor, BuildSettings, BuildTarget, resolve_build_settings};

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

const CARGO_CACHE_DIR_ENV: &str = "TRUEOS_BLUEPRINT_CARGO_CACHE_DIR";
const TARGET_SPEC_ENV: &str = "TRUEOS_BLUEPRINT_TARGET_SPEC";
const APPS_PUBLISH_SKIP_ENV: &str = "TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH";
const APPS_PUBLISH_MOUNT_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_MOUNT_URI";
const APPS_PUBLISH_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_URI";
const DEFAULT_APPS_PUBLISH_MOUNT_URI: &str = "smb://t4ce@pdjb/home-share";
const DEFAULT_APPS_PUBLISH_URI: &str = "smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps";
const RUSTFLAGS_ENCODED_SEPARATOR: char = '\u{1f}';
const TRUEOS_CHECK_CFG_FLAG: &str = "--check-cfg=cfg(target_os,values(\"trueos\",\"zkvm\"))";

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

            let package_dir = app_dir.join("apps").join(&example_name);
            let package_manifest = package_dir.join("Cargo.toml");
            if package_manifest.is_file() {
                build_one_target_to(
                    &package_dir,
                    &package_manifest,
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
    let mut cargo_profile = CargoProfile::Dev;
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

    let target_spec = default_target_spec(&app_dir)?;
    let target_name = target_spec
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("bad target spec path: {}", target_spec.display()))?
        .to_string();
    let packer_target_dir = app_dir.join("target").join("trueos-blueprint");
    let cargo_cache_root = cargo_cache_root(&packer_target_dir);
    let cargo_target_dir = cargo_cache_root
        .join(&target_name)
        .join(build_settings.flavor.cache_label());
    fs::create_dir_all(&cargo_target_dir).map_err(io_string)?;

    let work_dir = workdir(&packer_target_dir, &output_name)?;
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

    let mut cargo = Command::new("cargo");
    cargo
        .arg("+nightly")
        .arg("rustc")
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
                .map(|patch| patch.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    push_source_overlay_configs(&mut cargo, &staged_source_overlay);
    push_extra_rustflag(&mut cargo, TRUEOS_CHECK_CFG_FLAG);
    cargo.env("CARGO_TARGET_DIR", &cargo_target_dir);
    let declared_features = manifest_declared_features(&cargo_manifest_path)?;
    let mut extra_features = required_features.to_vec();
    for feature in &build_settings.extra_features {
        push_declared_feature(&mut extra_features, feature, &declared_features);
    }
    if !build_settings.has_global_allocator {
        push_declared_feature(
            &mut extra_features,
            "thin-default-global-allocator",
            &declared_features,
        );
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) && !build_settings.has_panic_handler
    {
        push_declared_feature(
            &mut extra_features,
            "thin-default-panic-handler",
            &declared_features,
        );
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) {
        cargo.arg("--no-default-features");
    } else if build_settings.needs_tokio_net {
        push_declared_feature(&mut extra_features, "tokio-net-probe", &declared_features);
    }
    if !extra_features.is_empty() {
        cargo.arg("--features").arg(extra_features.join(","));
    }
    if matches!(cargo_profile, CargoProfile::Release) {
        cargo.arg("--release");
    }

    match &build_target {
        BuildTarget::Package => {}
        BuildTarget::Example(name) => {
            cargo.arg("--example").arg(name);
        }
    };
    cargo.arg("--").arg("-Zno-link").arg("--emit=obj");

    println!("trueos-blueprint: cargo artifact profile: {}", cargo_profile.label());
    println!("trueos-blueprint: cargo artifact cache: {}", cargo_target_dir.display());
    run_command(&mut cargo, "cargo rustc")?;

    let target_dir = cargo_target_dir
        .join(&target_name)
        .join(cargo_profile.target_subdir());
    let deps_dir = target_dir.join("deps");
    if !deps_dir.is_dir() {
        return Err(format!("missing deps dir: {}", deps_dir.display()));
    }

    let app_obj = match &build_target {
        BuildTarget::Package => latest_cargo_object(&deps_dir, &cargo_artifact_stem(&output_name))?,
        BuildTarget::Example(name) => {
            latest_cargo_object(&target_dir.join("examples"), &cargo_artifact_stem(name))?
        }
    };
    let rlibs = collect_rlibs(&deps_dir)?;

    let linked = work_dir.join("module.o");
    let stripped = work_dir.join("module.stripped.o");

    let mut ld = tool_command(&["ld.lld", "rust-lld", "ld"])?;
    ld.arg("-r").arg("-o").arg(&linked).arg(&app_obj);
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

        match update.status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) if index + 1 == package_specs.len() => {
                return Err(format!("cargo update failed with status {status}"));
            }
            Ok(_) => {
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

fn publish_dist_blueprints(dist_dir: &Path) -> Result<(), String> {
    if env_flag_is_set(APPS_PUBLISH_SKIP_ENV) {
        println!("trueos-blueprint: skipping apps publish");
        return Ok(());
    }

    let target_uri =
        env_string(APPS_PUBLISH_URI_ENV).unwrap_or_else(|| DEFAULT_APPS_PUBLISH_URI.to_string());
    let mount_uri = env_string(APPS_PUBLISH_MOUNT_URI_ENV)
        .unwrap_or_else(|| DEFAULT_APPS_PUBLISH_MOUNT_URI.to_string());
    let bp_files = dist_blueprint_files(dist_dir)?;
    if bp_files.is_empty() {
        return Err(format!("no .bp files found in {}", dist_dir.display()));
    }

    println!("trueos-blueprint: publishing {} blueprints", bp_files.len());
    println!("trueos-blueprint: remote apps dir: {target_uri}");
    let mut mount = gio_command();
    mount.arg("mount").arg(&mount_uri);
    let _ = mount.status();

    ensure_remote_dir(&target_uri);
    clean_remote_dir(&target_uri)?;
    ensure_remote_dir(&target_uri);

    for bp_file in bp_files {
        let mut copy = gio_command();
        copy.arg("copy").arg(&bp_file).arg(&target_uri);
        run_command(&mut copy, "gio copy blueprint")?;
    }

    println!("trueos-blueprint: published dist blueprints");
    Ok(())
}

fn dist_blueprint_files(dist_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dist_dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("bp") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn clean_remote_dir(uri: &str) -> Result<(), String> {
    for child_uri in gio_list_uris(uri)? {
        clean_remote_entry(&child_uri)?;
    }
    Ok(())
}

fn clean_remote_entry(uri: &str) -> Result<(), String> {
    if let Ok(child_uris) = gio_list_uris(uri) {
        for child_uri in child_uris {
            clean_remote_entry(&child_uri)?;
        }
    }

    let mut remove = gio_command();
    remove.arg("remove").arg("-f").arg(uri);
    run_command(&mut remove, "gio remove remote app entry")
}

fn gio_list_uris(uri: &str) -> Result<Vec<String>, String> {
    let output = gio_command()
        .arg("list")
        .arg("-h")
        .arg("-u")
        .arg(uri)
        .output()
        .map_err(|err| format!("gio list failed to start: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gio list failed for {uri}: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn ensure_remote_dir(uri: &str) {
    let _ = gio_command().arg("mkdir").arg(uri).output();
}

fn gio_command() -> Command {
    let mut cmd = Command::new("gio");
    cmd.env_remove("GIO_MODULE_DIR");
    cmd
}

fn env_flag_is_set(name: &str) -> bool {
    env_string(name).is_some_and(|value| !matches!(value.as_str(), "0" | "false" | "FALSE"))
}

fn env_string(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    if value.is_empty() { None } else { Some(value) }
}

fn io_string(err: io::Error) -> String {
    err.to_string()
}

fn tool_command(tool_names: &[&str]) -> Result<Command, String> {
    let tool = find_tool(tool_names)?;
    Ok(Command::new(tool))
}

fn find_tool(tool_names: &[&str]) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Some(sysroot_bin) = rust_sysroot_bin_dir() {
        for tool_name in tool_names {
            candidates.push(sysroot_bin.join(tool_name));
            candidates.push(sysroot_bin.join("gcc-ld").join(tool_name));
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for tool_name in tool_names {
                candidates.push(dir.join(tool_name));
            }
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("missing required tool: {}", tool_names.join(" or ")))
}

fn rust_sysroot_bin_dir() -> Option<PathBuf> {
    let output = Command::new("rustc")
        .arg("+nightly")
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sysroot = String::from_utf8(output.stdout).ok()?;
    let sysroot = PathBuf::from(sysroot.trim());
    let host = env::var("HOST").ok().or_else(rustc_host_triple)?;
    Some(sysroot.join("lib").join("rustlib").join(host).join("bin"))
}

fn rustc_host_triple() -> Option<String> {
    let output = Command::new("rustc")
        .arg("+nightly")
        .arg("-vV")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout.lines().find_map(|line| {
        line.strip_prefix("host: ")
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn latest_cargo_object(dir: &Path, stem: &str) -> Result<PathBuf, String> {
    let prefix = format!("{stem}-");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(hash) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".o"))
        else {
            continue;
        };
        if hash.contains('.') {
            continue;
        }
        let modified = entry
            .metadata()
            .map_err(io_string)?
            .modified()
            .map_err(io_string)?;
        match &best {
            Some((best_modified, _)) if modified <= *best_modified => {}
            _ => best = Some((modified, path)),
        }
    }
    best.map(|(_, path)| path)
        .ok_or_else(|| format!("missing required build artifact for {stem} in {}", dir.display()))
}

fn cargo_artifact_stem(name: &str) -> String {
    name.replace('-', "_")
}

fn collect_rlibs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with("lib") && name.ends_with(".rlib") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
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

fn cargo_cache_root(default_packer_target_dir: &Path) -> PathBuf {
    env_path(CARGO_CACHE_DIR_ENV).unwrap_or_else(|| default_packer_target_dir.join("cargo-cache"))
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
    None
}

fn source_overlay_patches(
    app_dir: &Path,
    _manifest_path: &Path,
) -> Result<Vec<CratePatch>, String> {
    let mut out = Vec::new();

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

    if let Some(path) = find_vendor_dir(app_dir, "libc-0.2.185") {
        out.retain(|patch| patch.name != "libc");
        out.push(CratePatch {
            name: "libc".to_string(),
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
    let staged_manifest = staged_app_dir.join(
        manifest_path
            .file_name()
            .ok_or_else(|| format!("bad manifest path: {}", manifest_path.display()))?,
    );
    strip_manifest_patch_section(&staged_manifest)?;
    ensure_standalone_manifest_workspace(&staged_manifest)?;
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
        rewritten.push_str(
            "\n#[unsafe(no_mangle)]\npub extern \"C\" fn _start() -> ! {\n    main();\n    trueos::panic_abort(\"blueprint main returned\\n\")\n}\n",
        );
    }

    fs::write(&staged_source, rewritten).map_err(io_string)
}

struct LockMismatch {
    name: String,
    locked_version: String,
    overlay_version: String,
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
    metadata
        .current_dir(app_dir)
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
    let minor = req.minor.unwrap_or(0);
    let patch = req.patch.unwrap_or(0);

    if req.major > 0 {
        return SimpleVersion {
            major: req.major + 1,
            minor: 0,
            patch: 0,
        };
    }

    if minor > 0 {
        return SimpleVersion {
            major: 0,
            minor: minor + 1,
            patch: 0,
        };
    }

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
    let cargo_lock = fs::read_to_string(lock_path).map_err(io_string)?;
    let mut out = Vec::new();
    let mut in_package = false;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;

    for line in cargo_lock.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if in_package && let (Some(name), Some(version)) = (name.take(), version.take()) {
                out.push((name, version));
            }
            in_package = true;
            name = None;
            version = None;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            match key.trim() {
                "name" => name = toml_string_value(value.trim()),
                "version" => version = toml_string_value(value.trim()),
                _ => {}
            }
        }
    }

    if in_package && let (Some(name), Some(version)) = (name, version) {
        out.push((name, version));
    }
    Ok(out)
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

fn staged_source_overlay(source_overlay: &[CratePatch], work_dir: &Path) -> Vec<CratePatch> {
    source_overlay
        .iter()
        .map(|patch| CratePatch {
            name: patch.name.clone(),
            path: patch.path.clone(),
        })
        .collect()
}

fn staged_overlay_path(path: &Path, staging_root: &Path) -> PathBuf {
    let Some(name) = path.file_name() else {
        return path.to_path_buf();
    };
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    let Some(kind) = parent.file_name().and_then(|value| value.to_str()) else {
        return path.to_path_buf();
    };
    match kind {
        "vendor" => staging_root.join("vendor").join(name),
        "crates" => staging_root.join("crates").join(name),
        _ => path.to_path_buf(),
    }
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

fn push_extra_rustflag(command: &mut Command, flag: &str) {
    let mut encoded = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    if !encoded.is_empty() {
        encoded.push(RUSTFLAGS_ENCODED_SEPARATOR);
    }
    encoded.push_str(flag);
    command.env("CARGO_ENCODED_RUSTFLAGS", encoded);
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

fn package_app_specs(app_dir: &Path) -> Result<Vec<PackageAppSpec>, String> {
    let mut specs = Vec::new();
    let apps_dir = app_dir.join("apps");
    let scan_root = if apps_dir.is_dir() {
        &apps_dir
    } else {
        app_dir
    };
    for entry in fs::read_dir(scan_root).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let file_type = entry.file_type().map_err(io_string)?;
        if !file_type.is_dir() {
            continue;
        }

        let dir = entry.path();
        let manifest_path = dir.join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }

        let name = package_name(&manifest_path)?;
        specs.push(PackageAppSpec {
            name,
            dir,
            manifest_path,
        });
    }
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
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

fn push_declared_feature(features: &mut Vec<String>, feature: &str, declared_features: &[String]) {
    if declared_features.iter().any(|declared| declared == feature) {
        push_feature(features, feature);
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

fn entry_hint_hex(linked: &Path) -> String {
    if let Ok(mut readelf) = tool_command(&["llvm-readelf", "readelf"]) {
        if let Ok(output) = readelf.arg("-Ws").arg(linked).output() {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    let mut rust_main: Option<(u32, u32, usize)> = None;
                    for line in stdout.lines() {
                        let cols = line.split_whitespace().collect::<Vec<_>>();
                        if cols.len() < 8 {
                            continue;
                        }
                        if cols[3] != "FUNC" {
                            continue;
                        }
                        let Some(name) = cols.last().copied() else {
                            continue;
                        };
                        let value = cols[1].trim_start_matches("0x");
                        let section = cols[6].parse::<u32>().unwrap_or(0);
                        let value = u32::from_str_radix(value, 16).unwrap_or(0);
                        if name == "main" {
                            return format!("{section:08x}{value:08x}");
                        }
                        let prefer_rust_main = match &rust_main {
                            Some((_, _, best_len)) => name.len() < *best_len,
                            None => true,
                        };
                        if looks_like_rust_main_symbol(name) && prefer_rust_main {
                            rust_main = Some((section, value, name.len()));
                        }
                    }
                    if let Some((section, value, _)) = rust_main {
                        return format!("{section:08x}{value:08x}");
                    }
                }
            }
        }
    }

    if let Ok(mut readobj) = tool_command(&["llvm-readobj"]) {
        if let Ok(output) = readobj.arg("--symbols").arg(linked).output() {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    let mut current_value: Option<u32> = None;
                    let mut current_section: Option<u32> = None;
                    let mut current_is_function = false;
                    let mut current_name: Option<&str> = None;
                    let mut rust_main: Option<(u32, u32, usize)> = None;
                    for line in stdout.lines() {
                        let trimmed = line.trim();
                        if trimmed == "Symbol {" {
                            current_value = None;
                            current_section = None;
                            current_is_function = false;
                            current_name = None;
                            continue;
                        }
                        if trimmed == "}" {
                            if current_is_function {
                                if let Some(name) = current_name {
                                    let section = current_section.unwrap_or(0);
                                    let value = current_value.unwrap_or(0);
                                    if name == "main" {
                                        return format!("{section:08x}{value:08x}");
                                    }
                                    let prefer_rust_main = match &rust_main {
                                        Some((_, _, best_len)) => name.len() < *best_len,
                                        None => true,
                                    };
                                    if looks_like_rust_main_symbol(name) && prefer_rust_main {
                                        rust_main = Some((section, value, name.len()));
                                    }
                                }
                            }
                            continue;
                        }
                        if let Some(value) = trimmed.strip_prefix("Value: ") {
                            current_value =
                                u32::from_str_radix(value.trim_start_matches("0x"), 16).ok();
                            continue;
                        }
                        if let Some(section) = trimmed.strip_prefix("Section: ") {
                            let section = section
                                .rsplit_once('(')
                                .and_then(|(_, suffix)| suffix.strip_suffix(')'))
                                .map(str::trim)
                                .and_then(|value| value.strip_prefix("0x"))
                                .and_then(|value| u32::from_str_radix(value, 16).ok());
                            current_section = section;
                            continue;
                        }
                        if trimmed == "Type: Function" {
                            current_is_function = true;
                            continue;
                        }
                        if let Some(name) = trimmed.strip_prefix("Name: ") {
                            current_name = Some(name);
                        }
                    }
                    if let Some((section, value, _)) = rust_main {
                        return format!("{section:08x}{value:08x}");
                    }
                }
            }
        }
    }

    String::from("0000000000000000")
}

fn looks_like_rust_main_symbol(name: &str) -> bool {
    (name.starts_with("_R") && name.ends_with("4main"))
        || (name.starts_with("_ZN") && name.contains("4main17h") && name.ends_with('E'))
}

fn write_blueprint(out: &Path, stripped: &Path, entry_hint_hex: &str) -> Result<(), String> {
    let raw = fs::read(stripped).map_err(io_string)?;
    let payload = compress_blueprint_payload(stripped)?;
    let entry = u64::from_str_radix(entry_hint_hex, 16).map_err(|err| err.to_string())?;

    let mut bytes = Vec::with_capacity(24 + payload.len());
    bytes.extend_from_slice(b"TRBP");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&entry.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload);
    fs::write(out, bytes).map_err(io_string)
}

fn compress_blueprint_payload(stripped: &Path) -> Result<Vec<u8>, String> {
    let archive = stripped.with_extension("7z");
    let parent = stripped
        .parent()
        .ok_or_else(|| format!("missing parent dir for {}", stripped.display()))?;
    let file_name = stripped
        .file_name()
        .ok_or_else(|| format!("missing file name for {}", stripped.display()))?;

    let mut seven_zip = tool_command(&["7z", "7zz"])?;
    seven_zip
        .current_dir(parent)
        .arg("a")
        .arg("-t7z")
        .arg("-mx=9")
        .arg("-m0=LZMA2")
        .arg("-ms=off")
        .arg("-bd")
        .arg(&archive)
        .arg(file_name);
    run_command(&mut seven_zip, "7z")?;
    fs::read(&archive).map_err(io_string)
}

fn workdir(packer_target_dir: &Path, output_name: &str) -> Result<PathBuf, String> {
    Ok(packer_target_dir
        .join("work")
        .join(sanitize_path_component(output_name)))
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
