use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

mod abi_guard;
mod app_catalog;
mod artifact;
mod build_plan;
mod cargo_output;
mod cli;
mod publish;
mod toolchain;

#[cfg(test)]
mod test;

use app_catalog::{
    RustcTier, example_required_features, example_specs, manifest_declared_features,
    manifest_has_dependency, package_app_spec, package_app_specs, package_bin_name,
    package_blueprint_argv_entry_v1, package_blueprint_profile, package_blueprint_replicatable,
    package_blueprint_rustc_payload_dependencies, package_blueprint_rustc_tier, package_name,
    push_app_or_trueos_feature,
};
use artifact::{
    AssetBundleEntry, BLUEPRINT_CAP_ARGV_ENTRY_V1, BLUEPRINT_CAP_REPLICATABLE,
    attach_trueos_asset_bundle, cargo_artifact_stem, collect_rlibs_for_rlink, entry_hint_hex,
    entry_symbol_name, latest_cargo_root_artifacts, tool_command, verify_abort_panic_runtime,
    write_blueprint,
};
use build_plan::{BuildFlavor, BuildSettings, BuildTarget, resolve_build_settings};
use cargo_output::{
    print_cargo_output_notes, run_cargo_command, run_cargo_rustc_command,
    write_filtered_cargo_output,
};
use cli::{CargoProfile, PackageCatalog, parse_cli_args};
use publish::{publish_blueprint_file, publish_blueprint_files};

struct CratePatch {
    key: String,
    name: String,
    path: PathBuf,
}

impl CratePatch {
    fn new(name: impl Into<String>, path: PathBuf) -> Self {
        let name = name.into();
        Self {
            key: name.clone(),
            name,
            path,
        }
    }

    fn alias(key: impl Into<String>, name: impl Into<String>, path: PathBuf) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            path,
        }
    }
}

const CARGO_CACHE_DIR_ENV: &str = "TRUEOS_BLUEPRINT_CARGO_CACHE_DIR";
const TARGET_SPEC_ENV: &str = "TRUEOS_BLUEPRINT_TARGET_SPEC";
const TRUEOS_LIBC_VENDOR_DIR: &str = "libc-0.2.186";
const RUSTFLAGS_ENCODED_SEPARATOR: char = '\u{1f}';
const TRUEOS_CHECK_CFG_FLAG: &str = "--check-cfg=cfg(target_os,values(\"trueos\",\"zkvm\"))";
const BLUEPRINT_RUSTFLAGS: &[&str] =
    &[TRUEOS_CHECK_CFG_FLAG, "-A", "warnings", "-C", "panic=abort"];
const PUMPKIN_DEV_RUSTFLAGS: &[&str] = &["-Z", "threads=16"];
const REL_COALESCE_LINKER_SCRIPT: &str = r#"SECTIONS
{
  .text : { *(.text) *(.text.*) }
  .rodata : { *(.rodata) *(.rodata.*) }
  .data.rel.ro : { *(.data.rel.ro) *(.data.rel.ro.*) }
  .data : { *(.data) *(.data.*) }
  .bss : { *(.bss) *(.bss.*) *(COMMON) }
  .tdata : { *(.tdata) *(.tdata.*) }
  .tbss : { *(.tbss) *(.tbss.*) }
  .eh_frame : { *(.eh_frame) *(.eh_frame.*) }
  .gcc_except_table : { *(.gcc_except_table) *(.gcc_except_table.*) }
  .init_array : { *(.init_array) *(.init_array.*) }
  .fini_array : { *(.fini_array) *(.fini_array.*) }
}
"#;
const BLUEPRINT_VENDOR_PATCHES: &[(&str, &str)] = &[
    ("axum", "axum-0.8.9"),
    ("axum-core", "axum-core-0.5.6"),
    ("base64", "base64-0.22.1"),
    ("bytes", "bytes-1.11.1"),
    ("crc32fast", "crc32fast-1.5.0"),
    ("crossbeam-channel", "crossbeam-channel-0.5.15"),
    ("crossbeam-epoch", "crossbeam-epoch-0.9.18"),
    ("crossbeam-utils", "crossbeam-utils-0.8.21"),
    ("crossterm", "crossterm-0.29.0-trueos"),
    ("form_urlencoded", "form_urlencoded-1.2.2"),
    ("futures-core", "futures-core-0.3.32"),
    ("futures-task", "futures-task-0.3.32"),
    ("futures-util", "futures-util-0.3.32"),
    ("h2", "h2-0.4.14"),
    ("hickory-proto", "hickory-proto-0.25.2"),
    ("hickory-resolver", "hickory-resolver-0.25.2"),
    ("if-watch", "if-watch-3.2.2"),
    ("libloading", "libloading-0.8.9"),
    ("http", "http-1.4.0"),
    ("http-body", "http-body-1.0.1"),
    ("http-body-util", "http-body-util-0.1.3"),
    ("httpdate", "httpdate-1.0.3"),
    ("hyper", "hyper-1.9.0"),
    ("hyper-rustls", "hyper-rustls-0.27.9"),
    ("hyper-util", "hyper-util-0.1.20"),
    ("libp2p-mdns", "libp2p-mdns-0.48.0"),
    ("libp2p-quic", "libp2p-quic-0.13.1"),
    ("libp2p-tcp", "libp2p-tcp-0.44.1"),
    ("log", "log-0.4.32"),
    ("lua-src", "lua-src-550.1.1-trueos"),
    ("matchit", "matchit-0.8.4"),
    ("memchr", "memchr-2.8.2"),
    ("mime", "mime-0.3.17"),
    ("mio", "mio-1.2.0"),
    ("once_cell", "once_cell-1.21.4"),
    ("percent-encoding", "percent-encoding-2.3.2"),
    ("prism-q", "prism-q-0.20.0"),
    ("quinn", "quinn-0.11.9"),
    ("quinn-proto", "quinn-proto-0.11.14"),
    ("quinn-udp", "quinn-udp-0.5.14"),
    ("rand", "rand-0.8.6"),
    ("rayon", "rayon"),
    ("rayon-core", "rayon/rayon-core"),
    ("rustix", "rustix-1.1.4-trueos"),
    ("reqwest", "reqwest-0.13.3"),
    ("ring", "ring-0.17.14"),
    ("russh", "russh-0.62.4"),
    ("russh-cryptovec", "russh-cryptovec-0.62.0"),
    ("rustls-rustcrypto", "rustls-rustcrypto-0.0.2-alpha"),
    ("serde_urlencoded", "serde_urlencoded-0.7.1"),
    ("signal-hook-mio", "signal-hook-mio-0.2.5-trueos"),
    ("socket2", "socket2-0.6.3"),
    ("spin", "spin-0.10.0"),
    ("sync_wrapper", "sync_wrapper-1.0.2"),
    ("target-lexicon", "target-lexicon-0.13.5"),
    ("tokio", "tokio-1.52.3"),
    ("tokio-macros", "tokio-macros-2.7.0"),
    ("tokio-rustls", "tokio-rustls-0.26.4"),
    ("tokio-tungstenite", "tokio-tungstenite-0.29.0"),
    ("tokio-util", "tokio-util-0.7.18"),
    ("tower", "tower-0.5.3"),
    ("tower-http", "tower-http-0.6.9"),
    ("tower-layer", "tower-layer-0.3.3"),
    ("tower-service", "tower-service-0.3.3"),
    ("uzers", "uzers-0.12.2-trueos"),
    ("want", "want-0.3.1"),
];
const TRUEOS_IMAGE_CODEC_VENDOR_PATCHES: &[(&str, &str)] = &[
    ("crc32fast", "crc32fast-1.5.0"),
    ("fdeflate", "fdeflate-0.3.7"),
    ("png", "png-0.18.1"),
    ("simd-adler32", "simd-adler32-0.3.8"),
    ("zune-core", "zune-core-0.5.1"),
    ("zune-jpeg", "zune-jpeg-0.5.15"),
];

#[derive(Deserialize)]
struct BuildinsManifest {
    buildins: Vec<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("trueos-blueprint: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    toolchain::verify_rustc_identity()?;
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (app_dir, requested_apps, cargo_profile, package_catalog) = parse_cli_args(&args)?;
    let app_dir = fs::canonicalize(&app_dir)
        .map_err(|err| format!("failed to resolve app dir {}: {err}", app_dir.display()))?;
    let manifest_path = app_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("missing Cargo.toml in {}", app_dir.display()));
    }

    if package_name(&manifest_path)? == "trueos-blueprint" {
        if requested_apps.is_empty() {
            let buildins = if package_catalog == PackageCatalog::Apps {
                buildin_app_names(&app_dir)?
            } else {
                BTreeSet::new()
            };
            let examples = if package_catalog == PackageCatalog::Apps {
                example_specs(&manifest_path)?
                    .into_iter()
                    .filter(|example| !buildins.contains(example.name.as_str()))
                    .collect()
            } else {
                Vec::new()
            };
            let package_apps = package_app_specs(&app_dir, package_catalog)?
                .into_iter()
                .filter(|app| !buildins.contains(app.name.as_str()))
                .collect::<Vec<_>>();
            if package_catalog == PackageCatalog::Apps && !buildins.is_empty() {
                println!(
                    "trueos-blueprint: excluding {} build-in app(s) from bulk app build",
                    buildins.len()
                );
            }
            if examples.is_empty() && package_apps.is_empty() {
                if package_catalog == PackageCatalog::Apps {
                    println!("trueos-blueprint: no publishable non-build-in apps to build");
                    return Ok(());
                }
                build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Package,
                    &[],
                    cargo_profile,
                )?;
                return Ok(());
            }

            let mut built_blueprints = Vec::with_capacity(examples.len() + package_apps.len());
            for example in examples {
                built_blueprints.push(build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Example(example.name),
                    &example.required_features,
                    cargo_profile,
                )?);
            }

            for package_app in package_apps {
                println!(
                    "trueos-blueprint: package {}: {}",
                    package_catalog.item_label(),
                    package_app.name
                );
                built_blueprints.push(build_one_target_to(
                    &package_app.dir,
                    &package_app.manifest_path,
                    BuildTarget::Package,
                    &[],
                    &app_dir.join("dist"),
                    cargo_profile,
                )?);
            }

            publish_blueprint_files(&built_blueprints, package_catalog)?;
            return Ok(());
        }

        for example_name in requested_apps {
            if package_catalog == PackageCatalog::Apps
                && let Ok(required_features) =
                    example_required_features(&manifest_path, &example_name)
            {
                let bp_file = build_one_target(
                    &app_dir,
                    &manifest_path,
                    BuildTarget::Example(example_name),
                    &required_features,
                    cargo_profile,
                )?;
                publish_blueprint_file(&bp_file, package_catalog)?;
                continue;
            }

            if let Some(package_app) = package_app_spec(&app_dir, &example_name, package_catalog)? {
                let bp_file = build_one_target_to(
                    &package_app.dir,
                    &package_app.manifest_path,
                    BuildTarget::Package,
                    &[],
                    &app_dir.join("dist"),
                    cargo_profile,
                )?;
                publish_blueprint_file(&bp_file, package_catalog)?;
                continue;
            }

            return Err(match package_catalog {
                PackageCatalog::Apps => format!("unknown example or package `{example_name}`"),
                PackageCatalog::Probes => format!("unknown probe `{example_name}`"),
            });
        }
        return Ok(());
    }

    if !requested_apps.is_empty() {
        return Err("named apps are only supported from the trueos-blueprint root".to_string());
    }

    let build_target = BuildTarget::Package;
    let required_features = Vec::new();
    if let Some(root) = current_blueprint_root() {
        let bp_file = build_one_target_to(
            &app_dir,
            &manifest_path,
            build_target,
            &required_features,
            &root.join("dist"),
            cargo_profile,
        )?;
        publish_blueprint_file(&bp_file, package_catalog)
    } else {
        build_one_target(
            &app_dir,
            &manifest_path,
            build_target,
            &required_features,
            cargo_profile,
        )?;
        Ok(())
    }
}

fn build_one_target(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: BuildTarget,
    required_features: &[String],
    cargo_profile: CargoProfile,
) -> Result<PathBuf, String> {
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
) -> Result<PathBuf, String> {
    let mut capability_flags = if matches!(build_target, BuildTarget::Package)
        && package_blueprint_replicatable(manifest_path)?
    {
        BLUEPRINT_CAP_REPLICATABLE
    } else {
        0
    };
    let rustc_tier = if matches!(build_target, BuildTarget::Package) {
        package_blueprint_rustc_tier(manifest_path)?
    } else {
        None
    };
    if rustc_tier.is_some() || package_blueprint_argv_entry_v1(manifest_path)? {
        capability_flags |= BLUEPRINT_CAP_ARGV_ENTRY_V1;
    }
    let cargo_profile = if matches!(build_target, BuildTarget::Package) {
        package_blueprint_profile(manifest_path)?.unwrap_or(cargo_profile)
    } else {
        cargo_profile
    };
    let build_settings = resolve_build_settings(&app_dir, &manifest_path, &build_target)?;
    if matches!(build_settings.flavor, BuildFlavor::TokioStd) {
        ensure_rust_std_trueos_cfg_hooks()?;
        ensure_rust_std_trueos_thread_set_name()?;
        ensure_rust_std_trueos_thread_cleanup()?;
        ensure_rust_std_trueos_thread_current_rebind()?;
        ensure_rust_std_trueos_hash_random()?;
        ensure_rust_std_trueos_no_threads_tls()?;
        ensure_rust_std_trueos_no_backtrace()?;
    }

    let output_name = match &build_target {
        BuildTarget::Package => {
            package_bin_name(&manifest_path)?.unwrap_or(package_name(&manifest_path)?)
        }
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

    let source_overlay =
        source_overlay_patches(app_dir, manifest_path, &work_dir, &build_settings)?;
    if matches!(build_settings.flavor, BuildFlavor::TokioStd) {
        ensure_rust_src_libc_lock_matches_overlay(&source_overlay, &cargo_target_dir)?;
    }
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

    let mut cargo = toolchain::cargo_command();
    if let Some(manifest_dir) = cargo_manifest_path.parent() {
        cargo.current_dir(manifest_dir);
    }
    cargo
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
                .map(|patch| format!("{}={}", patch.key, patch.path.display()))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    push_source_overlay_configs(&mut cargo, &staged_source_overlay);
    push_extra_rustflags(&mut cargo, BLUEPRINT_RUSTFLAGS);
    if output_name == "pumpkin" && matches!(cargo_profile, CargoProfile::Dev) {
        // Keep Cargo from oversubscribing the host while rustc parallelizes
        // Pumpkin's unusually large crate frontend.
        cargo.arg("--jobs").arg("2");
        push_extra_rustflags(&mut cargo, PUMPKIN_DEV_RUSTFLAGS);
    }
    push_bindgen_clang_args(&mut cargo);
    push_trueos_cc_flags(&mut cargo);
    cargo.env("RUSTC_BOOTSTRAP_SYNTHETIC_TARGET", "1");
    cargo.env("CARGO_TARGET_DIR", &cargo_target_dir);
    if rustc_tier.is_some() {
        toolchain::configure_rustc_bootstrap_env(&mut cargo, &target_name)?;
    }
    let declared_features = manifest_declared_features(&cargo_manifest_path)?;
    let has_trueos_dependency = manifest_has_dependency(&cargo_manifest_path, "trueos")?;
    let mut extra_features = required_features.to_vec();
    for feature in &build_settings.features {
        push_app_or_trueos_feature(
            &mut extra_features,
            feature,
            &declared_features,
            has_trueos_dependency,
        );
    }
    if source_tree_mentions(app_dir, "trueos::ui3")? {
        push_app_or_trueos_feature(
            &mut extra_features,
            "ui3",
            &declared_features,
            has_trueos_dependency,
        );
    }
    if source_tree_mentions(app_dir, "trueos::platform::spawn_blocking")? {
        push_app_or_trueos_feature(
            &mut extra_features,
            "tokio-runtime",
            &declared_features,
            has_trueos_dependency,
        );
    }
    if !build_settings.source.declares_global_allocator && has_trueos_dependency {
        push_feature(&mut extra_features, "trueos/default-global-allocator");
    }
    let mut no_default_features = false;
    if declared_features
        .iter()
        .any(|feature| feature == "trueos-blueprint")
    {
        no_default_features = true;
        push_feature(&mut extra_features, "trueos-blueprint");
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd)
        && !build_settings.source.declares_panic_handler
        && has_trueos_dependency
    {
        push_feature(&mut extra_features, "trueos/default-panic-handler");
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) || is_helix_app_dir(app_dir) {
        no_default_features = true;
    } else if build_settings.source.uses_tokio_net {
        push_app_or_trueos_feature(
            &mut extra_features,
            "tokio-net-probe",
            &declared_features,
            has_trueos_dependency,
        );
    }
    if no_default_features {
        cargo.arg("--no-default-features");
    }
    if !extra_features.is_empty() {
        cargo.arg("--features").arg(extra_features.join(","));
    }
    if matches!(cargo_profile, CargoProfile::Release) {
        cargo.arg("--release");
    }

    match &build_target {
        BuildTarget::Package => {
            let has_explicit_bin = package_bin_name(manifest_path)?.is_some();
            let has_auto_main_bin = build_settings
                .source_path
                .file_name()
                .and_then(|name| name.to_str())
                == Some("main.rs");
            if has_explicit_bin || has_auto_main_bin {
                cargo.arg("--bin").arg(&output_name);
            }
        }
        BuildTarget::Example(name) => {
            cargo.arg("--example").arg(name);
        }
    };
    cargo.arg("--").arg("-Zno-link").arg("--emit=obj");

    println!(
        "trueos-blueprint: cargo artifact profile: {}",
        cargo_profile.label()
    );
    println!(
        "trueos-blueprint: cargo artifact cache: {}",
        cargo_target_dir.display()
    );
    let cargo_artifacts = run_cargo_rustc_command(&mut cargo, "cargo rustc", &deps_dir)?;

    if !deps_dir.is_dir() {
        return Err(format!("missing deps dir: {}", deps_dir.display()));
    }
    let rustc_payload = if rustc_tier.is_some() {
        let requested = package_blueprint_rustc_payload_dependencies(&cargo_manifest_path)?;
        if requested.is_empty() {
            RustcPayloadSelection::default()
        } else {
            let metadata = cargo_metadata_for_rustc_payload(
                &cargo_manifest_path,
                &target_spec,
                &staged_source_overlay,
                no_default_features,
                &extra_features,
            )?;
            select_rustc_payload(&metadata, &requested, &cargo_artifacts.target_metadata)?
        }
    } else {
        RustcPayloadSelection::default()
    };

    let root_artifacts = match &build_target {
        BuildTarget::Package => {
            latest_cargo_root_artifacts(&deps_dir, &cargo_artifact_stem(&output_name))?
        }
        BuildTarget::Example(name) => {
            latest_cargo_root_artifacts(&target_dir.join("examples"), &cargo_artifact_stem(name))?
        }
    };
    // Cargo's JSON stream describes every artifact produced or reused during
    // this invocation, including mutually exclusive build-std panic runtimes.
    // The root crate's `.rlink` is rustc's selected native link closure.
    let rlibs = collect_rlibs_for_rlink(&root_artifacts.rlink, &deps_dir)?;
    if matches!(build_settings.flavor, BuildFlavor::TokioStd) {
        verify_abort_panic_runtime(&rlibs)?;
    }

    let app_obj = if root_artifacts.objects.len() == 1 {
        root_artifacts.objects[0].clone()
    } else {
        println!(
            "trueos-blueprint: combining {} root codegen objects",
            root_artifacts.objects.len()
        );
        let combined = work_dir.join("app.codegen.o");
        let mut ld = tool_command(&["ld.lld", "rust-lld", "ld"])?;
        ld.arg("-r").arg("-o").arg(&combined);
        ld.args(&root_artifacts.objects);
        run_command(&mut ld, "root codegen-unit link")?;
        combined
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
    ld.arg("-r").arg("--gc-sections");
    if output_name == "pumpkin" && matches!(cargo_profile, CargoProfile::Dev) {
        let linker_script = work_dir.join("rel-coalesce.ld");
        fs::write(&linker_script, REL_COALESCE_LINKER_SCRIPT).map_err(io_string)?;
        ld.arg("-T").arg(&linker_script);
    }
    ld.arg("-o").arg(&linked);
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

    // A relocatable ELF import records only a symbol name, not its C function
    // type. Compare the source contracts while that information is still
    // available and refuse to package a register-layout mismatch.
    abi_guard::verify_before_pack(blueprint_root(app_dir).as_deref(), &linked)?;

    let entry_hint_hex = entry_hint_hex(&linked);

    let mut objcopy = tool_command(&["llvm-objcopy", "rust-objcopy", "objcopy"])?;
    objcopy.arg("--strip-debug").arg(&linked).arg(&stripped);
    run_command(&mut objcopy, "objcopy")?;

    let packaged_elf = match rustc_tier {
        Some(tier) => {
            let asset_elf = work_dir.join("module.assets.o");
            let assets = rustc_sysroot_asset_entries(
                tier,
                &target_spec,
                &target_name,
                &cargo_artifacts.sysroot_metadata,
                &rustc_payload,
            )?;
            println!(
                "trueos-blueprint: embedding native rustc sysroot tier={} files={}",
                rustc_tier_label(tier),
                assets.len()
            );
            attach_trueos_asset_bundle(&stripped, &asset_elf, &assets)?
        }
        None => stripped.clone(),
    };

    let out = output_dir.join(format!("{output_name}.bp"));
    fs::create_dir_all(out.parent().ok_or("bad output path")?).map_err(io_string)?;
    write_blueprint(&out, &packaged_elf, &entry_hint_hex, capability_flags)?;
    println!("packed {} -> {}", app_obj.display(), out.display());
    Ok(out)
}

fn rustc_tier_label(tier: RustcTier) -> &'static str {
    match tier {
        RustcTier::Min => "min",
    }
}

fn rustc_sysroot_asset_entries(
    tier: RustcTier,
    target_spec: &Path,
    target_name: &str,
    metadata: &[cargo_output::CargoTargetMetadataArtifact],
    payload: &RustcPayloadSelection,
) -> Result<Vec<AssetBundleEntry>, String> {
    let selected_metadata = match tier {
        // A compiler payload is built in the same Cargo target graph as the
        // rustc Blueprint. Cargo feature unification can therefore make an
        // otherwise no_std payload artifact depend on target `std` metadata
        // (for example through serde_core). Cargo metadata does not expose
        // the injected -Zbuild-std nodes, so retain the authenticated
        // current-invocation sysroot closure whenever a min Blueprint carries
        // compiler payloads.
        RustcTier::Min if !payload.artifacts.is_empty() => metadata.iter().collect(),
        RustcTier::Min => {
            let mut selected = Vec::with_capacity(3);
            for crate_name in ["core", "compiler_builtins", "alloc"] {
                let mut matches = metadata
                    .iter()
                    .filter(|artifact| artifact.crate_name == crate_name);
                let Some(artifact) = matches.next() else {
                    return Err(format!(
                        "native rustc min build produced no authenticated build-std \
                         {crate_name} metadata; refusing to package an unusable core sysroot"
                    ));
                };
                if matches.next().is_some() {
                    return Err(format!(
                        "native rustc min build produced ambiguous authenticated build-std \
                         {crate_name} metadata; refusing to guess"
                    ));
                }
                selected.push(artifact);
            }
            selected
        }
    };
    let target_file = target_spec
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "bad native rustc target spec path: {}",
                target_spec.display()
            )
        })?;
    let identity = toolchain::rustc_identity()?;
    let mut manifest = format!(
        "format=trueos-rustc-sysroot-v1\n\
         tier={}\n\
         toolchain={}\n\
         rustc={}\n\
         commit={}\n\
         commit-date={}\n\
         build-host={}\n\
         target={}\n\
         metadata-count={}\n\
         payload-metadata-count={}\n\
         payload-extern-count={}\n",
        rustc_tier_label(tier),
        toolchain::RUST_TOOLCHAIN,
        identity.version,
        identity.commit_hash,
        identity.commit_date,
        identity.host,
        target_name,
        selected_metadata.len(),
        payload.artifacts.len(),
        payload.direct_externs.len()
    );

    let target_bytes = fs::read(target_spec).map_err(io_string)?;
    let mut entries = Vec::with_capacity(selected_metadata.len() + payload.artifacts.len() + 3);
    entries.push(AssetBundleEntry::new(
        format!("rustc-sysroot/{target_file}"),
        target_bytes.clone(),
    ));
    // `--target rustc-sysroot/<triple>.json` supplies the requested target,
    // while rustc independently resolves its compiled host tuple when it
    // creates the session. Make the same authenticated specification
    // available at rustc's standard sysroot host-target fallback.
    entries.push(AssetBundleEntry::new(
        format!("rustc-sysroot/lib/rustlib/{target_name}/target.json"),
        target_bytes,
    ));
    for artifact in selected_metadata {
        let file_name = artifact
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "bad native rustc sysroot metadata path: {}",
                    artifact.path.display()
                )
            })?;
        if artifact
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("rmeta")
        {
            return Err(format!(
                "native rustc sysroot artifact is not metadata: {}",
                artifact.path.display()
            ));
        }
        manifest.push_str(&format!("metadata={} {}\n", artifact.crate_name, file_name));
        entries.push(AssetBundleEntry::new(
            format!("rustc-sysroot/lib/rustlib/{target_name}/lib/{file_name}"),
            fs::read(&artifact.path).map_err(io_string)?,
        ));
    }
    for artifact in &payload.artifacts {
        let file_name = artifact
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "bad rustc payload metadata path: {}",
                    artifact.path.display()
                )
            })?;
        if artifact
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("rmeta")
        {
            return Err(format!(
                "rustc payload artifact is not metadata: {}",
                artifact.path.display()
            ));
        }
        manifest.push_str(&format!("payload={} {}\n", artifact.crate_name, file_name));
        entries.push(AssetBundleEntry::new(
            format!("rustc-payload/lib/{file_name}"),
            fs::read(&artifact.path).map_err(io_string)?,
        ));
    }
    for direct in &payload.direct_externs {
        let file_name = direct
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "bad direct rustc payload metadata path: {}",
                    direct.path.display()
                )
            })?;
        manifest.push_str(&format!(
            "extern={} {} {}\n",
            direct.alias, direct.crate_name, file_name
        ));
    }
    entries.push(AssetBundleEntry::new(
        "rustc-sysroot/TRUEOS_SYSROOT_MANIFEST",
        manifest.into_bytes(),
    ));
    Ok(entries)
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

fn source_tree_mentions(app_dir: &Path, needle: &str) -> Result<bool, String> {
    let source_dir = app_dir.join("src");
    if !source_dir.is_dir() {
        return Ok(false);
    }
    source_tree_mentions_in_dir(&source_dir, needle)
}

fn source_tree_mentions_in_dir(dir: &Path, needle: &str) -> Result<bool, String> {
    for entry in fs::read_dir(dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_string)?;
        if file_type.is_dir() {
            if source_tree_mentions_in_dir(&path, needle)? {
                return Ok(true);
            }
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        if fs::read_to_string(&path)
            .map_err(io_string)?
            .contains(needle)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn run_staged_lock_overlay_update(
    staged_manifest: &Path,
    staged_source_overlay: &[CratePatch],
    mismatch: &LockMismatch,
) -> Result<(), String> {
    if staged_lock_overlay_aligned(staged_manifest, mismatch)? {
        return Ok(());
    }

    let package_specs = [
        format!("{}@{}", mismatch.name, mismatch.locked_version),
        mismatch.name.clone(),
    ];

    for (index, package_spec) in package_specs.iter().enumerate() {
        if staged_lock_overlay_aligned(staged_manifest, mismatch)? {
            return Ok(());
        }

        let mut update = toolchain::cargo_command();
        update
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
                if staged_lock_overlay_aligned(staged_manifest, mismatch)? {
                    return Ok(());
                }
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

fn staged_lock_overlay_aligned(
    staged_manifest: &Path,
    mismatch: &LockMismatch,
) -> Result<bool, String> {
    let lock_path = generated_lock_path(staged_manifest)?;
    let locked = lock_packages(&lock_path)?;
    let mut has_overlay_version = false;
    let mut has_same_semver_line = false;
    for package in locked
        .iter()
        .filter(|package| package.name == mismatch.name)
    {
        if cargo_semver_same_line(&package.version, &mismatch.overlay_version) {
            has_same_semver_line = true;
        }
        if package.version == mismatch.overlay_version {
            has_overlay_version = true;
        } else if cargo_semver_same_line(&package.version, &mismatch.overlay_version) {
            return Ok(false);
        }
    }
    // Earlier overlay updates can remove an optional or host-only dependency
    // from the staged graph entirely. In that case there is no remaining unit
    // on this semver line to repin.
    Ok(has_overlay_version || !has_same_semver_line)
}

fn enforce_source_overlay_lock(
    staged_manifest: &Path,
    staged_source_overlay: &[CratePatch],
) -> Result<(), String> {
    if staged_source_overlay.is_empty() {
        return Ok(());
    }

    let mut resolve = toolchain::cargo_command();
    resolve
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(staged_manifest);
    push_source_overlay_configs(&mut resolve, staged_source_overlay);
    let output = resolve
        .output()
        .map_err(|err| format!("cargo metadata failed to start: {err}"))?;
    let notes = write_filtered_cargo_output("cargo metadata", &[], &output.stderr)?;
    print_cargo_output_notes("cargo metadata", &notes);
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed with status {}",
            output.status
        ));
    }
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
        println!(
            "trueos-blueprint: source overlay lock forced: {}",
            forced.join(",")
        );
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

    if let Some(root) = current_blueprint_root() {
        for candidate in [
            root.join("target.json"),
            root.join("trueos.json"),
            root.join("trueos-app.json"),
            root.join("apps").join("target.json"),
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
    blueprint_root_from_ancestors(app_dir).or_else(current_blueprint_root)
}

fn current_blueprint_root() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    blueprint_root_from_ancestors(&cwd)
}

fn buildin_app_names(app_dir: &Path) -> Result<BTreeSet<String>, String> {
    let manifest_path = app_dir.join("buildins.json");
    if !manifest_path.is_file() {
        return Ok(BTreeSet::new());
    }
    let manifest: BuildinsManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(io_string)?,
    )
    .map_err(|err| format!("invalid {}: {err}", manifest_path.display()))?;
    Ok(manifest.buildins.into_iter().collect())
}

fn blueprint_root_from_ancestors(app_dir: &Path) -> Option<PathBuf> {
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

fn ensure_rust_std_trueos_thread_set_name() -> Result<(), String> {
    let unix_thread =
        toolchain::rust_sysroot()?.join("lib/rustlib/src/rust/library/std/src/sys/thread/unix.rs");
    let source = fs::read_to_string(&unix_thread).map_err(|err| {
        format!(
            "failed to read Rust std thread source {}; install rust-src or check permissions: {err}",
            unix_thread.display()
        )
    })?;
    if source.contains("target_os = \"trueos\"") {
        if source.contains("pub fn set_name(_name: &core::ffi::CStr)") {
            return Ok(());
        }
        if source.contains("pub fn set_name(_name: &CStr)") {
            let patched = source.replace(
                "pub fn set_name(_name: &CStr)",
                "pub fn set_name(_name: &core::ffi::CStr)",
            );
            fs::write(&unix_thread, patched).map_err(|err| {
                format!(
                    "failed to patch Rust std thread source {}: {err}",
                    unix_thread.display()
                )
            })?;
            println!(
                "trueos-blueprint: patched rust-src std unix thread set_name CStr path: {}",
                unix_thread.display()
            );
            return Ok(());
        }
    }
    if source.contains("target_os = \"trueos\"")
        && source.contains("pub fn set_name(_name: &core::ffi::CStr)")
    {
        return Ok(());
    }

    let marker = "\n#[cfg(not(target_os = \"espidf\"))]\npub fn sleep";
    let Some(marker_idx) = source.find(marker) else {
        return Err(format!(
            "failed to patch {}; missing std unix thread sleep marker",
            unix_thread.display()
        ));
    };
    let patch = r#"

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub fn set_name(_name: &core::ffi::CStr) {}
"#;
    let mut patched = String::with_capacity(source.len() + patch.len());
    patched.push_str(&source[..marker_idx]);
    patched.push_str(patch);
    patched.push_str(&source[marker_idx..]);
    fs::write(&unix_thread, patched).map_err(|err| {
        format!(
            "failed to patch Rust std thread source {}: {err}",
            unix_thread.display()
        )
    })?;
    println!(
        "trueos-blueprint: patched rust-src std unix thread set_name for trueos: {}",
        unix_thread.display()
    );
    Ok(())
}

fn pinned_rust_src_path(relative: &str) -> Result<PathBuf, String> {
    Ok(toolchain::rust_sysroot()?
        .join("lib/rustlib/src/rust/library")
        .join(relative))
}

const TRUEOS_STD_RANDOM_BACKEND: &str = r#"//! TRUEOS kernel random source.

unsafe extern "C" {
    fn sys_rand(recv_buf: *mut u32, words: usize);
}

pub fn fill_bytes(bytes: &mut [u8]) {
    let (pre, words, post) = unsafe { bytes.align_to_mut::<u32>() };
    if !words.is_empty() {
        unsafe {
            sys_rand(words.as_mut_ptr(), words.len());
        }
    }

    let mut buf = [0u32; 2];
    let len = (pre.len() + post.len() + size_of::<u32>() - 1) / size_of::<u32>();
    if len != 0 {
        unsafe { sys_rand(buf.as_mut_ptr(), len) };
    }

    let buf = buf.map(u32::to_ne_bytes);
    let buf = buf.as_flattened();
    pre.copy_from_slice(&buf[..pre.len()]);
    post.copy_from_slice(&buf[pre.len()..pre.len() + post.len()]);
}
"#;

fn ensure_rust_src_file(relative: &str, contents: &str, label: &str) -> Result<(), String> {
    let path = pinned_rust_src_path(relative)?;
    match fs::read_to_string(&path) {
        Ok(source) if source == contents => return Ok(()),
        Ok(_) => {
            return Err(format!(
                "failed to restore {}; existing {label} source differs from the pinned contents",
                path.display()
            ));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "failed to read Rust std source {}: {err}",
                path.display()
            ));
        }
    }

    fs::write(&path, contents)
        .map_err(|err| format!("failed to create Rust std source {}: {err}", path.display()))?;
    println!(
        "trueos-blueprint: restored rust-src TRUEOS {label}: {}",
        path.display()
    );
    Ok(())
}

fn ensure_rust_src_replacement(
    relative: &str,
    needle: &str,
    replacement: &str,
    label: &str,
) -> Result<(), String> {
    let path = pinned_rust_src_path(relative)?;
    let source = fs::read_to_string(&path).map_err(|err| {
        format!(
            "failed to read Rust std source {}; install rust-src or check permissions: {err}",
            path.display()
        )
    })?;
    if source.contains(replacement) {
        return Ok(());
    }
    if !source.contains(needle) {
        return Err(format!(
            "failed to patch {}; missing {label} marker after nightly update",
            path.display()
        ));
    }

    let patched = source.replacen(needle, replacement, 1);
    fs::write(&path, patched)
        .map_err(|err| format!("failed to patch Rust std source {}: {err}", path.display()))?;
    println!(
        "trueos-blueprint: restored rust-src TRUEOS {label} hook: {}",
        path.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_cfg_hooks() -> Result<(), String> {
    ensure_rust_src_file(
        "std/src/sys/random/trueos.rs",
        TRUEOS_STD_RANDOM_BACKEND,
        "random backend",
    )?;
    ensure_rust_src_replacement(
        "std/src/os/mod.rs",
        "        #[cfg(target_os = \"linux\")]\n        pub mod linux;",
        "        #[cfg(any(target_os = \"linux\", target_os = \"trueos\"))]\n        pub mod linux;",
        "os::linux module",
    )?;
    ensure_rust_src_replacement(
        "std/src/os/mod.rs",
        "#[cfg(any(target_os = \"linux\", target_os = \"android\", target_os = \"cygwin\", doc))]\nmod net;",
        "#[cfg(any(target_os = \"linux\", target_os = \"trueos\", target_os = \"android\", target_os = \"cygwin\", doc))]\nmod net;",
        "os::net module",
    )?;
    ensure_rust_src_replacement(
        "std/src/os/unix/mod.rs",
        "    #[cfg(target_os = \"linux\")]\n    pub use crate::os::linux::*;",
        "    #[cfg(any(target_os = \"linux\", target_os = \"trueos\"))]\n    pub use crate::os::linux::*;",
        "Unix platform alias",
    )?;
    ensure_rust_src_replacement(
        "std/src/os/linux/mod.rs",
        "pub mod fs;\npub mod net;\npub mod process;\npub mod raw;",
        "pub mod fs;\n#[cfg(not(target_os = \"trueos\"))]\npub mod net;\n#[cfg(not(target_os = \"trueos\"))]\npub mod process;\npub mod raw;",
        "Linux compatibility modules",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/random/mod.rs",
        "    target_os = \"trusty\" => {\n        mod trusty;\n        pub use trusty::fill_bytes;\n    }",
        "    target_os = \"trueos\" => {\n        mod trueos;\n        pub use trueos::fill_bytes;\n    }\n    target_os = \"trusty\" => {\n        mod trusty;\n        pub use trusty::fill_bytes;\n    }",
        "random backend selection",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/random/mod.rs",
        "        all(target_family = \"wasm\", target_os = \"unknown\"),\n        target_os = \"trueos\",\n        target_os = \"xous\",",
        "        all(target_family = \"wasm\", target_os = \"unknown\"),\n        target_os = \"xous\",",
        "unsupported random fallback exclusion",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/random/mod.rs",
        "#[cfg(not(any(\n    target_os = \"linux\",\n    target_os = \"trueos\",\n    target_os = \"android\",",
        "#[cfg(not(any(\n    target_os = \"linux\",\n    target_os = \"android\",",
        "generic HashMap random keys",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/args/unix.rs",
        "#[cfg(any(\n    target_os = \"linux\",\n    target_os = \"android\",",
        "#[cfg(any(\n    target_os = \"linux\",\n    target_os = \"trueos\",\n    target_os = \"android\",",
        "process arguments",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/paths/unix.rs",
        "#[cfg(any(\n    target_os = \"linux\",\n    target_os = \"cygwin\",",
        "#[cfg(any(\n    target_os = \"linux\",\n    target_os = \"trueos\",\n    target_os = \"cygwin\",",
        "current executable path",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/fs/unix.rs",
        "        target_os = \"l4re\",\n        target_os = \"linux\",\n        target_os = \"nto\",\n        target_os = \"qnx\",\n        target_os = \"redox\",",
        "        target_os = \"l4re\",\n        target_os = \"linux\",\n        target_os = \"trueos\",\n        target_os = \"nto\",\n        target_os = \"qnx\",\n        target_os = \"redox\",",
        "directory metadata",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/thread_local/mod.rs",
        "        target_os = \"uefi\",\n        target_os = \"zkvm\",",
        "        target_os = \"uefi\",\n        target_os = \"trueos\",\n        target_os = \"zkvm\",",
        "no-threads TLS selection",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/pal/unix/time.rs",
        "    pub fn now(clock: libc::clockid_t) -> Timespec {\n        use crate::mem::MaybeUninit;",
        r#"    pub fn now(clock: libc::clockid_t) -> Timespec {
        #[cfg(target_os = "trueos")]
        {
            unsafe extern "Rust" {
                fn trueos_platform_monotonic_nanos() -> u64;
                fn trueos_platform_unix_seconds() -> u64;
            }

            let nanos = if clock == libc::CLOCK_MONOTONIC {
                unsafe { trueos_platform_monotonic_nanos() }
            } else {
                unsafe { trueos_platform_unix_seconds() }.saturating_mul(NSEC_PER_SEC)
            };
            let secs = (nanos / NSEC_PER_SEC).min(i64::MAX as u64) as i64;
            let nsec = (nanos % NSEC_PER_SEC) as i64;
            return Timespec::new(secs, nsec).unwrap_or_else(|_| Timespec::zero());
        }

        use crate::mem::MaybeUninit;"#,
        "platform clock",
    )?;
    Ok(())
}

fn ensure_rust_src_libc_lock_matches_overlay(
    source_overlay: &[CratePatch],
    cargo_target_dir: &Path,
) -> Result<(), String> {
    let Some(libc_patch) = source_overlay.iter().find(|patch| patch.name == "libc") else {
        return Ok(());
    };
    let Some(overlay_version) = package_version(&libc_patch.path.join("Cargo.toml"))? else {
        return Err(format!(
            "failed to read patched libc version from {}",
            libc_patch.path.display()
        ));
    };

    let rust_src_lock = pinned_rust_src_path("Cargo.lock")?;
    let locked = lock_packages(&rust_src_lock)?;
    let locked_version = locked
        .iter()
        .find(|package| package.name == "libc")
        .map(|package| package.version.as_str());
    if locked_version == Some(overlay_version.as_str()) {
        return Ok(());
    }

    let rust_src_library = rust_src_lock.parent().ok_or_else(|| {
        format!(
            "bad nightly rust-src Cargo.lock path: {}",
            rust_src_lock.display()
        )
    })?;
    println!(
        "trueos-blueprint: rust-src libc lock {} -> {} for TRUEOS build-std",
        locked_version.unwrap_or("missing"),
        overlay_version
    );

    let mut update = toolchain::cargo_command();
    update
        .current_dir(rust_src_library)
        .arg("update")
        .arg("-p")
        .arg(match locked_version {
            Some(version) => format!("libc@{version}"),
            None => String::from("libc"),
        })
        .arg("--precise")
        .arg(&overlay_version);
    run_cargo_command(&mut update, "rust-src libc lock update")?;

    let updated = lock_packages(&rust_src_lock)?;
    if !updated
        .iter()
        .any(|package| package.name == "libc" && package.version == overlay_version)
    {
        return Err(format!(
            "rust-src libc lock update did not select patched version {} in {}",
            overlay_version,
            rust_src_lock.display()
        ));
    }

    if cargo_target_dir.exists() {
        fs::remove_dir_all(cargo_target_dir).map_err(|err| {
            format!(
                "failed to invalidate TRUEOS build-std cache {}: {err}",
                cargo_target_dir.display()
            )
        })?;
    }
    fs::create_dir_all(cargo_target_dir).map_err(io_string)?;
    println!(
        "trueos-blueprint: invalidated TRUEOS build-std cache after libc repin: {}",
        cargo_target_dir.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_thread_cleanup() -> Result<(), String> {
    let unix_thread = pinned_rust_src_path("std/src/sys/thread/unix.rs")?;
    let source = fs::read_to_string(&unix_thread).map_err(|err| {
        format!(
            "failed to read Rust std thread source {}; install rust-src or check permissions: {err}",
            unix_thread.display()
        )
    })?;
    if source.contains("TRUEOS service-lane pthread shim has no native TLS destructor") {
        return Ok(());
    }

    let needle = r#"                rust_start();
            }
            ptr::null_mut()"#;
    let replacement = r#"                rust_start();

                #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                {
                    // TRUEOS service-lane pthread shim has no native TLS destructor.
                    crate::rt::thread_cleanup();
                }
            }
            ptr::null_mut()"#;
    if !source.contains(needle) {
        return Err(format!(
            "failed to patch {}; missing std unix thread_start cleanup marker",
            unix_thread.display()
        ));
    }
    let patched = source.replace(needle, replacement);
    fs::write(&unix_thread, patched).map_err(|err| {
        format!(
            "failed to patch Rust std thread source {}: {err}",
            unix_thread.display()
        )
    })?;
    println!(
        "trueos-blueprint: patched rust-src std unix thread cleanup for trueos: {}",
        unix_thread.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_thread_current_rebind() -> Result<(), String> {
    let current_rs = pinned_rust_src_path("std/src/thread/current.rs")?;
    let source = fs::read_to_string(&current_rs).map_err(|err| {
        format!(
            "failed to read Rust std current thread source {}; install rust-src or check permissions: {err}",
            current_rs.display()
        )
    })?;
    if source.contains("TRUEOS carrier lanes may host multiple logical std threads") {
        return Ok(());
    }

    let needle = r#"pub(super) fn set_current(thread: Thread) -> Result<(), Thread> {
    if CURRENT.get() != NONE {
        return Err(thread);
    }

    match id::get() {
        Some(id) if id == thread.id() => {}
        None => id::set(thread.id()),
        _ => return Err(thread),
    }

    // Make sure that `crate::rt::thread_cleanup` will be run, which will
    // call `drop_current`.
    crate::sys::thread_local::guard::enable();
    CURRENT.set(thread.into_raw().cast_mut());
    Ok(())
}"#;
    let replacement = r#"pub(super) fn set_current(thread: Thread) -> Result<(), Thread> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        // TRUEOS carrier lanes may host multiple logical std threads over time.
        // Rebind the per-lane std thread handle/id instead of treating a prior
        // logical thread as a fatal TLS collision.
        let current = CURRENT.get();
        if current > DESTROYED {
            unsafe {
                drop(Thread::from_raw(current));
            }
        }
        id::set(thread.id());
        crate::sys::thread_local::guard::enable();
        CURRENT.set(thread.into_raw().cast_mut());
        return Ok(());
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        if CURRENT.get() != NONE {
            return Err(thread);
        }

        match id::get() {
            Some(id) if id == thread.id() => {}
            None => id::set(thread.id()),
            _ => return Err(thread),
        }

        // Make sure that `crate::rt::thread_cleanup` will be run, which will
        // call `drop_current`.
        crate::sys::thread_local::guard::enable();
        CURRENT.set(thread.into_raw().cast_mut());
        Ok(())
    }
}"#;
    if !source.contains(needle) {
        return Err(format!(
            "failed to patch {}; missing std current thread set_current marker",
            current_rs.display()
        ));
    }
    let patched = source.replace(needle, replacement);
    fs::write(&current_rs, patched).map_err(|err| {
        format!(
            "failed to patch Rust std current thread source {}: {err}",
            current_rs.display()
        )
    })?;
    println!(
        "trueos-blueprint: patched rust-src std current thread rebind for trueos: {}",
        current_rs.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_hash_random() -> Result<(), String> {
    let random_rs = pinned_rust_src_path("std/src/hash/random.rs")?;
    let source = fs::read_to_string(&random_rs).map_err(|err| {
        format!(
            "failed to read Rust std hash random source {}; install rust-src or check permissions: {err}",
            random_rs.display()
        )
    })?;
    if source.contains("TRUEOS_HASH_RANDOM_COUNTER") {
        return Ok(());
    }

    let needle = r#"        thread_local!(static KEYS: Cell<(u64, u64)> = {
            Cell::new(hashmap_random_keys())
        });

        KEYS.with(|keys| {
            let (k0, k1) = keys.get();
            keys.set((k0.wrapping_add(1), k1));
            RandomState { k0, k1 }
        })"#;
    let replacement = r#"        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            use crate::sync::atomic::{AtomicU64, Ordering};

            static TRUEOS_HASH_RANDOM_COUNTER: AtomicU64 = AtomicU64::new(0);

            let seed = TRUEOS_HASH_RANDOM_COUNTER.fetch_add(1, Ordering::Relaxed);
            let (mut k0, mut k1) = hashmap_random_keys();
            k0 = k0.wrapping_add(seed);
            k1 = k1.wrapping_add(seed.rotate_left(32));
            RandomState { k0, k1 }
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
            thread_local!(static KEYS: Cell<(u64, u64)> = {
                Cell::new(hashmap_random_keys())
            });

            KEYS.with(|keys| {
                let (k0, k1) = keys.get();
                keys.set((k0.wrapping_add(1), k1));
                RandomState { k0, k1 }
            })
        }"#;
    if !source.contains(needle) {
        return Err(format!(
            "failed to patch {}; missing std hash RandomState thread-local marker",
            random_rs.display()
        ));
    }
    let patched = source.replace(needle, replacement);
    fs::write(&random_rs, patched).map_err(|err| {
        format!(
            "failed to patch Rust std hash random source {}: {err}",
            random_rs.display()
        )
    })?;
    println!(
        "trueos-blueprint: patched rust-src std HashMap RandomState for trueos: {}",
        random_rs.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_no_threads_tls() -> Result<(), String> {
    let no_threads_rs = pinned_rust_src_path("std/src/sys/thread_local/no_threads.rs")?;
    let mut source = fs::read_to_string(&no_threads_rs).map_err(|err| {
        format!(
            "failed to read Rust std no_threads TLS source {}; install rust-src or check permissions: {err}",
            no_threads_rs.display()
        )
    })?;
    let original = source.clone();
    source = source.replace(
        "#[cfg(target_has_threads)]\ncompile_error!(\"Using no_threads implementation on a target with threads\");",
        "#[cfg(all(\n    target_has_threads,\n    not(any(target_os = \"trueos\", target_os = \"zkvm\"))\n))]\ncompile_error!(\"Using no_threads implementation on a target with threads\");",
    );
    if source.contains("TRUEOS_STD_NO_THREADS_PER_SLOT") {
        source = source.replace(
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 128;",
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 4096;",
        );
        source = source.replace(
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 64;",
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 4096;",
        );
        source = source.replace(
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 1024;",
            "const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 4096;",
        );
        source = source.replace(
            r#"unsafe extern "Rust" {
    fn trueos_tokio_tls_current_slot() -> u32;
}"#,
            r#"unsafe extern "C" {
    fn trueos_cabi_wls_current_slot() -> u32;
}"#,
        );
        source = source.replace(
            "let slot = unsafe { trueos_tokio_tls_current_slot() } as usize;",
            "let slot = unsafe { trueos_cabi_wls_current_slot() } as usize;",
        );
        source = source.replace(
            r#"                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::EagerStorage<$t> =
                    $crate::thread::local_impl::EagerStorage { value: __RUST_STD_INTERNAL_INIT };
                &__RUST_STD_INTERNAL_VAL.value"#,
            r#"                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::LazyStorage<$t> =
                    $crate::thread::local_impl::LazyStorage::new();
                __RUST_STD_INTERNAL_VAL.get(None, || __RUST_STD_INTERNAL_INIT)"#,
        );
        source = source.replace(
            r#"        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        unsafe {
            $crate::thread::LocalKey::new(|_| {
                $(#[$align_attr])*
                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::LazyStorage<$t> =
                    $crate::thread::local_impl::LazyStorage::new();
                __RUST_STD_INTERNAL_VAL.get(None, || __RUST_STD_INTERNAL_INIT)
            })
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        unsafe {
            $crate::thread::LocalKey::new(|_| {
                $(#[$align_attr])*
                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::EagerStorage<$t> =
                    $crate::thread::local_impl::EagerStorage { value: __RUST_STD_INTERNAL_INIT };
                &__RUST_STD_INTERNAL_VAL.value
            })
        }"#,
            r#"        // NOTE: Please update the shadowing test in `tests/thread.rs` if these types are renamed.
        unsafe {
            $crate::thread::LocalKey::new(|_| {
                $(#[$align_attr])*
                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::LazyStorage<$t> =
                    $crate::thread::local_impl::LazyStorage::new();
                __RUST_STD_INTERNAL_VAL.get(None, || __RUST_STD_INTERNAL_INIT)
            })
        }"#,
        );
        source = source.replace(
            "#[cfg(not(any(target_os = \"trueos\", target_os = \"zkvm\")))]\n#[allow(missing_debug_implementations)]\n#[repr(transparent)] // Required for correctness of `#[rustc_align_static]`\npub struct EagerStorage<T>",
            "#[allow(missing_debug_implementations)]\n#[repr(transparent)] // Required for correctness of `#[rustc_align_static]`\npub struct EagerStorage<T>",
        );
        source = source.replace(
            "// SAFETY: the target doesn't have threads.\n#[cfg(not(any(target_os = \"trueos\", target_os = \"zkvm\")))]\nunsafe impl<T> Sync for EagerStorage<T> {}",
            "// SAFETY: the target doesn't have threads.\nunsafe impl<T> Sync for EagerStorage<T> {}",
        );
        if source != original {
            fs::write(&no_threads_rs, source).map_err(|err| {
                format!(
                    "failed to upgrade Rust std no_threads TLS source {}: {err}",
                    no_threads_rs.display()
                )
            })?;
            println!(
                "trueos-blueprint: upgraded rust-src std no_threads TLS for TRUEOS WLS: {}",
                no_threads_rs.display()
            );
        }
        return Ok(());
    }

    fn replace_required(
        source: &mut String,
        path: &Path,
        needle: &str,
        replacement: &str,
        marker: &str,
    ) -> Result<(), String> {
        if !source.contains(needle) {
            return Err(format!(
                "failed to patch {}; missing std no_threads TLS marker: {marker}",
                path.display()
            ));
        }
        *source = source.replace(needle, replacement);
        Ok(())
    }

    replace_required(
        &mut source,
        &no_threads_rs,
        r#"use crate::cell::{Cell, UnsafeCell};
use crate::mem::MaybeUninit;
use crate::ptr;"#,
        r#"#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::cell::{Cell, UnsafeCell};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::mem::MaybeUninit;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::{
    boxed::Box,
    marker::PhantomData,
    sync::atomic::{AtomicUsize, Ordering},
};
use crate::ptr;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const TRUEOS_STD_NO_THREADS_PER_SLOT: usize = 4096;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "C" {
    fn trueos_cabi_wls_current_slot() -> u32;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn trueos_std_thread_local_slot() -> usize {
    let slot = unsafe { trueos_cabi_wls_current_slot() } as usize;
    if slot < TRUEOS_STD_NO_THREADS_PER_SLOT {
        slot
    } else {
        0
    }
}"#,
        "imports",
    )?;

    replace_required(
        &mut source,
        &no_threads_rs,
        r#"            $crate::thread::LocalKey::new(|_| {
                $(#[$align_attr])*
                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::EagerStorage<$t> =
                    $crate::thread::local_impl::EagerStorage { value: __RUST_STD_INTERNAL_INIT };
                &__RUST_STD_INTERNAL_VAL.value
            })"#,
        r#"            $crate::thread::LocalKey::new(|_| {
                $(#[$align_attr])*
                static __RUST_STD_INTERNAL_VAL: $crate::thread::local_impl::LazyStorage<$t> =
                    $crate::thread::local_impl::LazyStorage::new();
                __RUST_STD_INTERNAL_VAL.get(None, || __RUST_STD_INTERNAL_INIT)
            })"#,
        "const eager storage",
    )?;

    replace_required(
        &mut source,
        &no_threads_rs,
        r#"#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Alive,
    Destroying,
}

#[allow(missing_debug_implementations)]
#[repr(C)]
pub struct LazyStorage<T> {
    // This field must be first, for correctness of `#[rustc_align_static]`
    value: UnsafeCell<MaybeUninit<T>>,
    state: Cell<State>,
}

impl<T> LazyStorage<T> {
    pub const fn new() -> LazyStorage<T> {
        LazyStorage {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: Cell::new(State::Initial),
        }
    }

    /// Gets a pointer to the TLS value, potentially initializing it with the
    /// provided parameters.
    ///
    /// The resulting pointer may not be used after reentrant inialialization
    /// has occurred.
    #[inline]
    pub fn get(&'static self, i: Option<&mut Option<T>>, f: impl FnOnce() -> T) -> *const T {
        if self.state.get() == State::Alive {
            self.value.get() as *const T
        } else {
            self.initialize(i, f)
        }
    }

    #[cold]
    fn initialize(&'static self, i: Option<&mut Option<T>>, f: impl FnOnce() -> T) -> *const T {
        let value = i.and_then(Option::take).unwrap_or_else(f);

        // Destroy the old value if it is initialized
        // FIXME(#110897): maybe panic on recursive initialization.
        if self.state.get() == State::Alive {
            self.state.set(State::Destroying);
            // Safety: we check for no initialization during drop below
            unsafe {
                ptr::drop_in_place(self.value.get() as *mut T);
            }
            self.state.set(State::Initial);
        }

        // Guard against initialization during drop
        if self.state.get() == State::Destroying {
            panic!("Attempted to initialize thread-local while it is being dropped");
        }

        unsafe {
            self.value.get().write(MaybeUninit::new(value));
        }
        self.state.set(State::Alive);

        self.value.get() as *const T
    }
}

// SAFETY: the target doesn't have threads.
unsafe impl<T> Sync for LazyStorage<T> {}"#,
        r#"#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Alive,
    Destroying,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(missing_debug_implementations)]
pub struct LazyStorage<T> {
    slots: [AtomicUsize; TRUEOS_STD_NO_THREADS_PER_SLOT],
    _marker: PhantomData<fn() -> T>,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T> LazyStorage<T> {
    pub const fn new() -> LazyStorage<T> {
        LazyStorage {
            slots: [const { AtomicUsize::new(0) }; TRUEOS_STD_NO_THREADS_PER_SLOT],
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn get(&'static self, i: Option<&mut Option<T>>, f: impl FnOnce() -> T) -> *const T {
        let slot = trueos_std_thread_local_slot();
        let cell = &self.slots[slot];
        let existing = cell.load(Ordering::Acquire);
        if existing != 0 {
            return existing as *const T;
        }

        self.initialize(cell, i, f)
    }

    #[cold]
    fn initialize(
        &'static self,
        cell: &AtomicUsize,
        i: Option<&mut Option<T>>,
        f: impl FnOnce() -> T,
    ) -> *const T {
        let value = i.and_then(Option::take).unwrap_or_else(f);
        let ptr = Box::leak(Box::new(value)) as *mut T as usize;
        match cell.compare_exchange(0, ptr, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => ptr as *const T,
            Err(existing) => {
                unsafe {
                    drop(Box::from_raw(ptr as *mut T));
                }
                existing as *const T
            }
        }
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
#[allow(missing_debug_implementations)]
#[repr(C)]
pub struct LazyStorage<T> {
    // This field must be first, for correctness of `#[rustc_align_static]`
    value: UnsafeCell<MaybeUninit<T>>,
    state: Cell<State>,
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl<T> LazyStorage<T> {
    pub const fn new() -> LazyStorage<T> {
        LazyStorage {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: Cell::new(State::Initial),
        }
    }

    /// Gets a pointer to the TLS value, potentially initializing it with the
    /// provided parameters.
    ///
    /// The resulting pointer may not be used after reentrant inialialization
    /// has occurred.
    #[inline]
    pub fn get(&'static self, i: Option<&mut Option<T>>, f: impl FnOnce() -> T) -> *const T {
        if self.state.get() == State::Alive {
            self.value.get() as *const T
        } else {
            self.initialize(i, f)
        }
    }

    #[cold]
    fn initialize(&'static self, i: Option<&mut Option<T>>, f: impl FnOnce() -> T) -> *const T {
        let value = i.and_then(Option::take).unwrap_or_else(f);

        // Destroy the old value if it is initialized
        // FIXME(#110897): maybe panic on recursive initialization.
        if self.state.get() == State::Alive {
            self.state.set(State::Destroying);
            // Safety: we check for no initialization during drop below
            unsafe {
                ptr::drop_in_place(self.value.get() as *mut T);
            }
            self.state.set(State::Initial);
        }

        // Guard against initialization during drop
        if self.state.get() == State::Destroying {
            panic!("Attempted to initialize thread-local while it is being dropped");
        }

        unsafe {
            self.value.get().write(MaybeUninit::new(value));
        }
        self.state.set(State::Alive);

        self.value.get() as *const T
    }
}

// SAFETY: the TRUEOS variant uses per-slot atomics and leaked values.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl<T> Sync for LazyStorage<T> {}

// SAFETY: the target doesn't have threads.
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
unsafe impl<T> Sync for LazyStorage<T> {}"#,
        "lazy storage",
    )?;

    replace_required(
        &mut source,
        &no_threads_rs,
        r#"pub(crate) struct LocalPointer {
    p: Cell<*mut ()>,
}

impl LocalPointer {
    pub const fn __new() -> LocalPointer {
        LocalPointer { p: Cell::new(ptr::null_mut()) }
    }

    pub fn get(&self) -> *mut () {
        self.p.get()
    }

    pub fn set(&self, p: *mut ()) {
        self.p.set(p)
    }
}

// SAFETY: the target doesn't have threads.
unsafe impl Sync for LocalPointer {}"#,
        r#"#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) struct LocalPointer {
    slots: [AtomicUsize; TRUEOS_STD_NO_THREADS_PER_SLOT],
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl LocalPointer {
    pub const fn __new() -> LocalPointer {
        LocalPointer {
            slots: [const { AtomicUsize::new(0) }; TRUEOS_STD_NO_THREADS_PER_SLOT],
        }
    }

    pub fn get(&self) -> *mut () {
        self.slots[trueos_std_thread_local_slot()].load(Ordering::Acquire) as *mut ()
    }

    pub fn set(&self, p: *mut ()) {
        self.slots[trueos_std_thread_local_slot()].store(p as usize, Ordering::Release)
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub(crate) struct LocalPointer {
    p: Cell<*mut ()>,
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl LocalPointer {
    pub const fn __new() -> LocalPointer {
        LocalPointer { p: Cell::new(ptr::null_mut()) }
    }

    pub fn get(&self) -> *mut () {
        self.p.get()
    }

    pub fn set(&self, p: *mut ()) {
        self.p.set(p)
    }
}

// SAFETY: the TRUEOS variant uses per-slot atomics.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe impl Sync for LocalPointer {}

// SAFETY: the target doesn't have threads.
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
unsafe impl Sync for LocalPointer {}"#,
        "local pointer",
    )?;

    fs::write(&no_threads_rs, source).map_err(|err| {
        format!(
            "failed to patch Rust std no_threads TLS source {}: {err}",
            no_threads_rs.display()
        )
    })?;
    println!(
        "trueos-blueprint: patched rust-src std no_threads TLS for trueos: {}",
        no_threads_rs.display()
    );
    Ok(())
}

fn ensure_rust_std_trueos_no_backtrace() -> Result<(), String> {
    ensure_rust_src_replacement(
        "std/src/sys/backtrace.rs",
        r#"    pub(crate) fn print(&mut self, w: &mut dyn Write, format: PrintFmt) -> io::Result<()> {
        // There are issues currently linking libbacktrace into tests, and in"#,
        r#"    pub(crate) fn print(&mut self, w: &mut dyn Write, format: PrintFmt) -> io::Result<()> {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            let _ = (w, format);
            return Ok(());
        }

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        {
        // There are issues currently linking libbacktrace into tests, and in"#,
        "panic backtrace exclusion start",
    )?;
    ensure_rust_src_replacement(
        "std/src/sys/backtrace.rs",
        r#"        write!(w, "{}", DisplayBacktrace { format })
    }
}

/// # Safety"#,
        r#"        write!(w, "{}", DisplayBacktrace { format })
        }
    }
}

/// # Safety"#,
        "panic backtrace exclusion end",
    )
}

fn find_vendor_dir(app_dir: &Path, name: &str) -> Option<PathBuf> {
    find_blueprint_vendor_dir(app_dir, name)
}

fn find_trueos_kernel_vendor_dir(app_dir: &Path, name: &str) -> Option<PathBuf> {
    let kernel_manifest = trueos_kernel_manifest(app_dir)?;
    let candidate = kernel_manifest.parent()?.join("vendor").join(name);
    candidate.is_dir().then_some(candidate)
}

fn find_blueprint_vendor_dir(app_dir: &Path, name: &str) -> Option<PathBuf> {
    for ancestor in app_dir.ancestors() {
        let candidate = ancestor.join("vendor").join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    if let Some(root) = current_blueprint_root() {
        let candidate = root.join("vendor").join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn source_overlay_patches(
    app_dir: &Path,
    manifest_path: &Path,
    work_dir: &Path,
    build_settings: &BuildSettings,
) -> Result<Vec<CratePatch>, String> {
    let mut out = Vec::new();

    if is_helix_app_dir(app_dir) {
        return Ok(out);
    }

    ensure_overlay_registry_sources(app_dir, manifest_path, work_dir)?;

    // Crossterm 0.29, Rustix 1.1, and signal-hook-mio are checked-in
    // Blueprint platform vendors. Crossterm 0.28 and Rustix 0.38 still need
    // isolated overlays because their ABI-compatible platform forks have not
    // been promoted yet.
    let uses_crossterm = manifest_or_lock_requires_crossterm_overlays(app_dir, manifest_path)?;
    let uses_crossterm_0_28 =
        uses_crossterm && manifest_or_lock_requires_crossterm_0_28_overlay(app_dir, manifest_path)?;
    let uses_rustix =
        uses_crossterm || manifest_or_lock_mentions_crate(app_dir, manifest_path, "rustix")?;
    let uses_rustix_0_38 =
        uses_rustix && manifest_or_lock_requires_rustix_0_38_overlay(app_dir, manifest_path)?;
    // Tonic's `channel` transport feature pulls hyper-timeout transitively.
    // Select both overlays from the direct Tonic declaration so packages
    // without a local Cargo.lock still materialize the TRUEOS I/O types.
    let uses_tonic = manifest_or_lock_mentions_crate(app_dir, manifest_path, "tonic")?;
    let uses_png = manifest_or_lock_mentions_crate(app_dir, manifest_path, "png")?;
    let uses_zune_jpeg = manifest_or_lock_mentions_crate(app_dir, manifest_path, "zune-jpeg")?;

    add_blueprint_vendor_patches(app_dir, &mut out);
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) {
        add_trueos_image_codec_vendor_patches(app_dir, &mut out, uses_png, uses_zune_jpeg);
    }

    let libc_path = find_blueprint_vendor_dir(app_dir, TRUEOS_LIBC_VENDOR_DIR).ok_or_else(|| {
        format!(
            "missing required TRUEOS libc overlay vendor/{TRUEOS_LIBC_VENDOR_DIR}; restore the complete TRUEOS-Blueprints checkout"
        )
    })?;
    out.retain(|patch| patch.name != "libc");
    out.push(CratePatch::new("libc", libc_path));

    if matches!(build_settings.flavor, BuildFlavor::TokioStd)
        && manifest_or_lock_mentions_crate(app_dir, manifest_path, "futures-timer")?
    {
        out.retain(|patch| patch.name != "futures-timer");
        out.push(CratePatch::new(
            "futures-timer",
            stage_futures_timer_trueos_overlay(work_dir)?,
        ));
    }

    if manifest_or_lock_mentions_crate(app_dir, manifest_path, "tokio-stream")? {
        out.retain(|patch| patch.name != "tokio-stream");
        out.push(CratePatch::new(
            "tokio-stream",
            stage_tokio_stream_trueos_overlay(work_dir)?,
        ));
    }

    if let Some(path) = find_vendor_dir(app_dir, "hyper-rustls-0.27.9") {
        out.retain(|patch| patch.name != "hyper-rustls");
        out.push(CratePatch::new("hyper-rustls", path));
    }

    if uses_tonic || manifest_or_lock_mentions_crate(app_dir, manifest_path, "hyper-timeout")? {
        out.retain(|patch| patch.name != "hyper-timeout");
        out.push(CratePatch::new(
            "hyper-timeout",
            stage_hyper_timeout_trueos_overlay(work_dir)?,
        ));
    }

    if uses_tonic {
        out.retain(|patch| patch.name != "tonic");
        out.push(CratePatch::new(
            "tonic",
            stage_tonic_trueos_overlay(work_dir)?,
        ));
    }

    if matches!(build_settings.flavor, BuildFlavor::TokioStd) {
        out.retain(|patch| patch.name != "hyper-util");
        out.push(CratePatch::new(
            "hyper-util",
            stage_hyper_util_tokio_std_overlay(app_dir, work_dir)?,
        ));
        out.retain(|patch| patch.name != "tower");
        out.push(CratePatch::new(
            "tower",
            stage_tower_tokio_std_overlay(app_dir, work_dir)?,
        ));
    }

    add_getrandom_source_overlays(app_dir, &mut out);

    if manifest_or_lock_mentions_crate(app_dir, manifest_path, "socket2")? {
        let socket2_0_5 = stage_socket2_trueos_overlay(work_dir, "0.5.10")?;
        out.push(CratePatch::alias("socket2_0_5", "socket2", socket2_0_5));
    }

    if manifest_or_lock_mentions_crate(app_dir, manifest_path, "ctrlc")? {
        let path = stage_ctrlc_trueos_overlay(work_dir)?;
        out.retain(|patch| patch.name != "ctrlc");
        out.push(CratePatch::new("ctrlc", path));
    }

    if manifest_or_lock_mentions_crate(app_dir, manifest_path, "argmax")? {
        let path = stage_argmax_trueos_overlay(work_dir)?;
        out.retain(|patch| patch.name != "argmax");
        out.push(CratePatch::new("argmax", path));
    }

    if uses_rustix_0_38 {
        let rustix_0_38 = stage_rustix_trueos_overlay(work_dir, "0.38.44")?;
        out.retain(|patch| patch.key != "rustix_0_38");
        out.push(CratePatch::alias("rustix_0_38", "rustix", rustix_0_38));
    }

    if uses_crossterm_0_28 {
        let crossterm_0_28 = stage_crossterm_trueos_overlay(work_dir, "0.28.1")?;
        out.retain(|patch| patch.key != "crossterm_0_28");
        out.push(CratePatch::alias(
            "crossterm_0_28",
            "crossterm",
            crossterm_0_28,
        ));
    }

    add_manifest_path_patches(manifest_path, &mut out)?;

    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Preserve explicit local source patches while still routing them through
/// the Blueprint packer's audited `--config patch.crates-io` mechanism.
///
/// Staged manifests deliberately have their patch section stripped. Without
/// materializing path patches here, an external engine workspace can compile
/// against one patched crate while a transitive dependency resolves a second,
/// nominally equal crates.io copy. Rust then treats their public types as
/// unrelated. Only local path patches are admitted; git/registry replacement
/// policy remains owned by the packer.
fn add_manifest_path_patches(
    manifest_path: &Path,
    patches: &mut Vec<CratePatch>,
) -> Result<(), String> {
    let manifest_dir = manifest_path.parent().unwrap_or(manifest_path);
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_patch = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_patch = trimmed == "[patch.crates-io]";
            continue;
        }
        if !in_patch || trimmed.is_empty() {
            continue;
        }
        let Some((key, dependency_path)) = inline_dependency_name_and_path(line) else {
            continue;
        };
        let dependency_path = PathBuf::from(dependency_path);
        let resolved = if dependency_path.is_absolute() {
            dependency_path
        } else {
            manifest_dir.join(dependency_path)
        };
        let canonical = fs::canonicalize(&resolved).map_err(|error| {
            format!(
                "failed to resolve path patch `{key}` at {}: {error}",
                resolved.display()
            )
        })?;
        let package = package_name(&canonical.join("Cargo.toml"))?;
        patches.retain(|patch| patch.key != key && patch.name != package);
        patches.push(if package == key {
            CratePatch::new(key, canonical)
        } else {
            CratePatch::alias(key, package, canonical)
        });
    }
    Ok(())
}

fn add_blueprint_vendor_patches(app_dir: &Path, patches: &mut Vec<CratePatch>) {
    for (name, vendor_dir) in BLUEPRINT_VENDOR_PATCHES {
        let Some(path) = find_blueprint_vendor_dir(app_dir, vendor_dir) else {
            continue;
        };
        patches.retain(|patch| patch.name != *name && patch.key != *name);
        patches.push(CratePatch::new(*name, path));
    }
}

fn add_trueos_image_codec_vendor_patches(
    app_dir: &Path,
    patches: &mut Vec<CratePatch>,
    uses_png: bool,
    uses_zune_jpeg: bool,
) {
    for (name, vendor_dir) in TRUEOS_IMAGE_CODEC_VENDOR_PATCHES {
        let selected = match *name {
            "crc32fast" | "fdeflate" | "png" | "simd-adler32" => uses_png,
            "zune-core" | "zune-jpeg" => uses_zune_jpeg,
            _ => false,
        };
        if !selected {
            continue;
        }
        let Some(path) = find_trueos_kernel_vendor_dir(app_dir, vendor_dir) else {
            continue;
        };
        patches.retain(|patch| patch.name != *name && patch.key != *name);
        patches.push(CratePatch::new(*name, path));
    }
}

fn add_getrandom_source_overlays(app_dir: &Path, patches: &mut Vec<CratePatch>) {
    patches.retain(|patch| patch.name != "getrandom");
    for (key, vendor_dir) in [
        ("getrandom", "getrandom-0.2.17"),
        ("getrandom_03", "getrandom-0.3.4"),
        ("getrandom_04", "getrandom-0.4.2"),
    ] {
        let Some(path) = find_vendor_dir(app_dir, vendor_dir) else {
            continue;
        };
        patches.push(if key == "getrandom" {
            CratePatch::new("getrandom", path)
        } else {
            CratePatch::alias(key, "getrandom", path)
        });
    }
}

fn manifest_or_lock_mentions_crate(
    app_dir: &Path,
    manifest_path: &Path,
    crate_name: &str,
) -> Result<bool, String> {
    let needle = format!("{crate_name}");
    let manifest = fs::read_to_string(manifest_path).map_err(io_string)?;
    if manifest.lines().any(|line| {
        line.split('#')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with(&needle)
    }) {
        return Ok(true);
    }

    let lock_path = app_dir.join("Cargo.lock");
    if !lock_path.is_file() {
        return Ok(false);
    }
    let lock = fs::read_to_string(lock_path).map_err(io_string)?;
    Ok(lock
        .lines()
        .any(|line| line.trim() == format!("name = \"{crate_name}\"")))
}

fn manifest_or_lock_requires_crossterm_overlays(
    app_dir: &Path,
    manifest_path: &Path,
) -> Result<bool, String> {
    if manifest_or_lock_mentions_crate(app_dir, manifest_path, "crossterm")? {
        return Ok(true);
    }

    let manifest = fs::read_to_string(manifest_path).map_err(io_string)?;
    Ok(manifest.lines().any(|line| {
        line.split('#')
            .next()
            .unwrap_or("")
            .split(['\'', '"'])
            .any(|value| value == "crossterm" || value.ends_with("/crossterm"))
    }))
}

fn manifest_or_lock_requires_crossterm_0_28_overlay(
    app_dir: &Path,
    manifest_path: &Path,
) -> Result<bool, String> {
    let lock_path = app_dir.join("Cargo.lock");
    if lock_path.is_file() {
        let packages = lock_packages(&lock_path)?;
        let crossterm_versions = packages
            .iter()
            .filter(|package| package.name == "crossterm")
            .map(|package| package.version.as_str())
            .collect::<Vec<_>>();
        if !crossterm_versions.is_empty() {
            return Ok(crossterm_versions
                .iter()
                .any(|version| version.starts_with("0.28.")));
        }
    }

    let manifest = fs::read_to_string(manifest_path).map_err(io_string)?;
    let direct_crossterm = manifest.lines().find(|line| {
        line.split('#')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with("crossterm")
    });
    Ok(!direct_crossterm.is_some_and(|line| line.contains("0.29")))
}

fn manifest_or_lock_requires_rustix_0_38_overlay(
    app_dir: &Path,
    manifest_path: &Path,
) -> Result<bool, String> {
    let lock_path = app_dir.join("Cargo.lock");
    if lock_path.is_file() {
        let packages = lock_packages(&lock_path)?;
        let rustix_versions = packages
            .iter()
            .filter(|package| package.name == "rustix")
            .map(|package| package.version.as_str())
            .collect::<Vec<_>>();
        if !rustix_versions.is_empty() {
            return Ok(rustix_versions
                .iter()
                .any(|version| version.starts_with("0.38.")));
        }
    }

    let manifest = fs::read_to_string(manifest_path).map_err(io_string)?;
    let direct_rustix = manifest.lines().find(|line| {
        line.split('#')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with("rustix")
    });
    Ok(!direct_rustix.is_some_and(|line| line.contains("1.")))
}

fn stage_ctrlc_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const CTRL_C_VERSION: &str = "3.4.4";
    let source_name = format!("ctrlc-{CTRL_C_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(source_name.as_str());
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_ctrlc_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_argmax_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const ARGMAX_VERSION: &str = "0.4.0";
    let source_name = format!("argmax-{ARGMAX_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(source_name.as_str());
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_argmax_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_rustix_trueos_overlay(work_dir: &Path, version: &str) -> Result<PathBuf, String> {
    let source_name = format!("rustix-{version}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(format!("rustix-{version}-trueos"));
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_rustix_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_socket2_trueos_overlay(work_dir: &Path, version: &str) -> Result<PathBuf, String> {
    let source_name = format!("socket2-{version}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(format!("socket2-{version}-trueos"));
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_socket2_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_futures_timer_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const FUTURES_TIMER_VERSION: &str = "3.0.4";
    let source_name = format!("futures-timer-{FUTURES_TIMER_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join("futures-timer-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_futures_timer_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_crossterm_trueos_overlay(work_dir: &Path, version: &str) -> Result<PathBuf, String> {
    let source_name = format!("crossterm-{version}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(format!("crossterm-{version}-trueos"));
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_crossterm_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_tokio_stream_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const TOKIO_STREAM_VERSION: &str = "0.1.17";
    let source_name = format!("tokio-stream-{TOKIO_STREAM_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join("tokio-stream-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_tokio_stream_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_hyper_timeout_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const HYPER_TIMEOUT_VERSION: &str = "0.5.2";
    let source_name = format!("hyper-timeout-{HYPER_TIMEOUT_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join("hyper-timeout-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_hyper_timeout_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_tonic_trueos_overlay(work_dir: &Path) -> Result<PathBuf, String> {
    const TONIC_VERSION: &str = "0.14.6";
    let source_name = format!("tonic-{TONIC_VERSION}");
    let source = find_cargo_registry_crate(&source_name).ok_or_else(|| {
        format!(
            "missing Cargo registry source for {source_name}; run `cargo fetch` for the app once"
        )
    })?;
    let staged = work_dir.join("source-overlay-crates").join("tonic-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_tonic_trueos_overlay(&staged)?;
    Ok(staged)
}

fn stage_hyper_util_tokio_std_overlay(app_dir: &Path, work_dir: &Path) -> Result<PathBuf, String> {
    let source = find_vendor_dir(app_dir, "hyper-util-0.1.20")
        .ok_or_else(|| "missing vendored hyper-util-0.1.20 source".to_string())?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join("hyper-util-tokio-std-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_hyper_util_tokio_std_overlay(&staged)?;
    Ok(staged)
}

fn stage_tower_tokio_std_overlay(app_dir: &Path, work_dir: &Path) -> Result<PathBuf, String> {
    let source = find_vendor_dir(app_dir, "tower-0.5.3")
        .ok_or_else(|| "missing vendored tower-0.5.3 source".to_string())?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join("tower-tokio-std-trueos");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    patch_tower_tokio_std_overlay(&staged)?;
    Ok(staged)
}

fn find_cargo_registry_crate(crate_dir_name: &str) -> Option<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))?;
    let registry_src = cargo_home.join("registry").join("src");
    let entries = fs::read_dir(registry_src).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(crate_dir_name);
        if candidate.join("Cargo.toml").is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ensure_overlay_registry_sources(
    app_dir: &Path,
    manifest_path: &Path,
    work_dir: &Path,
) -> Result<(), String> {
    let uses_crossterm = manifest_or_lock_requires_crossterm_overlays(app_dir, manifest_path)?;
    let uses_crossterm_0_28 =
        uses_crossterm && manifest_or_lock_requires_crossterm_0_28_overlay(app_dir, manifest_path)?;
    let uses_rustix =
        uses_crossterm || manifest_or_lock_mentions_crate(app_dir, manifest_path, "rustix")?;
    let uses_rustix_0_38 =
        uses_rustix && manifest_or_lock_requires_rustix_0_38_overlay(app_dir, manifest_path)?;
    let uses_tonic = manifest_or_lock_mentions_crate(app_dir, manifest_path, "tonic")?;
    let registry_overlays = [
        ("argmax", "0.4.0", "argmax-0.4.0"),
        ("crossterm", "0.28.1", "crossterm-0.28.1"),
        ("ctrlc", "3.4.4", "ctrlc-3.4.4"),
        ("futures-timer", "3.0.4", "futures-timer-3.0.4"),
        ("hyper-timeout", "0.5.2", "hyper-timeout-0.5.2"),
        ("rustix", "0.38.44", "rustix-0.38.44"),
        ("socket2", "0.5.10", "socket2-0.5.10"),
        ("tokio-stream", "0.1.17", "tokio-stream-0.1.17"),
        ("tonic", "0.14.6", "tonic-0.14.6"),
    ];
    let mut missing = Vec::new();
    for (crate_name, version, source_dir) in registry_overlays {
        let required_by_crossterm = (uses_crossterm_0_28
            && matches!((crate_name, source_dir), ("crossterm", "crossterm-0.28.1")))
            || (uses_rustix_0_38
                && matches!((crate_name, source_dir), ("rustix", "rustix-0.38.44")));
        let required_by_tonic = uses_tonic && crate_name == "hyper-timeout";
        if find_cargo_registry_crate(source_dir).is_none()
            && (required_by_crossterm
                || required_by_tonic
                || manifest_or_lock_mentions_crate(app_dir, manifest_path, crate_name)?)
        {
            missing.push((crate_name, version, source_dir));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }

    println!(
        "trueos-blueprint: fetching missing overlay sources: {}",
        missing
            .iter()
            .map(|(_, _, source_dir)| *source_dir)
            .collect::<Vec<_>>()
            .join(",")
    );
    let fetch_dir = work_dir.join("registry-fetch");
    fs::create_dir_all(&fetch_dir).map_err(io_string)?;
    let mut fetch_manifest = String::from(
        "[package]\nname = \"trueos-overlay-source-fetch\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n\n[workspace]\n\n[dependencies]\n",
    );
    for (index, (crate_name, version, _)) in missing.iter().enumerate() {
        fetch_manifest.push_str(&format!(
            "overlay_{index} = {{ package = {}, version = {} }}\n",
            toml_string(crate_name),
            toml_string(&format!("={version}")),
        ));
    }
    let fetch_manifest_path = fetch_dir.join("Cargo.toml");
    fs::write(&fetch_manifest_path, fetch_manifest).map_err(io_string)?;
    fs::write(fetch_dir.join("lib.rs"), "").map_err(io_string)?;
    let mut fetch = toolchain::cargo_command();
    fetch
        .arg("fetch")
        .arg("--manifest-path")
        .arg(&fetch_manifest_path);
    run_command(&mut fetch, "cargo fetch overlay sources")?;

    let still_missing = missing
        .into_iter()
        .map(|(_, _, source_dir)| source_dir)
        .filter(|source_dir| find_cargo_registry_crate(source_dir).is_none())
        .collect::<Vec<_>>();
    if still_missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "cargo fetch completed without required overlay sources: {}",
            still_missing.join(",")
        ))
    }
}

fn patch_tokio_stream_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/wrappers/tcp_listener.rs"),
        "use std::io;",
        "use tokio::io;",
    )?;
    replace_file_text(
        &crate_dir.join("src/wrappers.rs"),
        "    #[cfg(unix)]\n    mod unix_listener;\n    #[cfg(unix)]\n    pub use unix_listener::UnixListenerStream;",
        "    #[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\n    mod unix_listener;\n    #[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\n    pub use unix_listener::UnixListenerStream;",
    )
}

fn patch_futures_timer_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        "#[cfg(not(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\")))]\nmod native;\n#[cfg(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\"))]\nmod wasm;\n\n#[cfg(not(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\")))]\npub use self::native::Delay;\n#[cfg(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\"))]\npub use self::wasm::Delay;",
        "#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\nmod trueos;\n#[cfg(all(\n    not(any(target_os = \"trueos\", target_os = \"zkvm\")),\n    not(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\"))\n))]\nmod native;\n#[cfg(all(\n    not(any(target_os = \"trueos\", target_os = \"zkvm\")),\n    target_arch = \"wasm32\",\n    feature = \"wasm-bindgen\"\n))]\nmod wasm;\n\n#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\npub use self::trueos::Delay;\n#[cfg(all(\n    not(any(target_os = \"trueos\", target_os = \"zkvm\")),\n    not(all(target_arch = \"wasm32\", feature = \"wasm-bindgen\"))\n))]\npub use self::native::Delay;\n#[cfg(all(\n    not(any(target_os = \"trueos\", target_os = \"zkvm\")),\n    target_arch = \"wasm32\",\n    feature = \"wasm-bindgen\"\n))]\npub use self::wasm::Delay;",
    )?;
    append_if_missing(
        &crate_dir.join("Cargo.toml"),
        "dependencies.trueos-tokio",
        "\n[target.\"cfg(any(target_os = \\\"trueos\\\", target_os = \\\"zkvm\\\"))\".dependencies.trueos-tokio]\npackage = \"tokio\"\nversion = \"=1.52.3\"\ndefault-features = false\nfeatures = [\"rt\", \"time\"]\n",
    )?;
    fs::write(crate_dir.join("src/trueos.rs"), FUTURES_TIMER_TRUEOS_RS).map_err(io_string)
}

fn patch_tonic_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/transport/channel/service/io.rs"),
        "use std::io::{self, IoSlice};",
        "use hyper::io::{self, IoSlice};",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/incoming.rs"),
        "    net::{SocketAddr, TcpListener as StdTcpListener},",
        "    net::SocketAddr,",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/incoming.rs"),
        "    time::Duration,\n};\n\nuse socket2::TcpKeepalive;",
        "    time::Duration,\n};\n\nuse socket2::TcpKeepalive;\nuse tokio::io;",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/incoming.rs"),
        "    pub fn bind(addr: SocketAddr) -> std::io::Result<Self> {\n        let std_listener = StdTcpListener::bind(addr)?;\n        std_listener.set_nonblocking(true)?;\n\n        Ok(TcpListener::from_std(std_listener)?.into())\n    }",
        "    pub async fn bind(addr: SocketAddr) -> io::Result<Self> {\n        TcpListener::bind(addr).await.map(Into::into)\n    }",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/incoming.rs"),
        "    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {",
        "    pub fn local_addr(&self) -> io::Result<SocketAddr> {",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/incoming.rs"),
        "    type Item = std::io::Result<TcpStream>;",
        "    type Item = io::Result<TcpStream>;",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/mod.rs"),
        "#[cfg(unix)]\nmod unix;",
        "#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\nmod unix;",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/mod.rs"),
        "#[cfg(unix)]\npub use unix::UdsConnectInfo;",
        "#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\npub use unix::UdsConnectInfo;",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/mod.rs"),
        "    fn bind_incoming(&self, addr: SocketAddr) -> Result<TcpIncoming, super::Error> {\n        Ok(TcpIncoming::bind(addr)\n            .map_err(super::Error::from_source)?\n            .with_nodelay(Some(self.tcp_nodelay))\n            .with_keepalive(self.tcp_keepalive)\n            .with_keepalive_interval(self.tcp_keepalive_interval)\n            .with_keepalive_retries(self.tcp_keepalive_retries))\n    }",
        "    async fn bind_incoming(&self, addr: SocketAddr) -> Result<TcpIncoming, super::Error> {\n        Ok(TcpIncoming::bind(addr)\n            .await\n            .map_err(super::Error::from_source)?\n            .with_nodelay(Some(self.tcp_nodelay))\n            .with_keepalive(self.tcp_keepalive)\n            .with_keepalive_interval(self.tcp_keepalive_interval)\n            .with_keepalive_retries(self.tcp_keepalive_retries))\n    }",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/server/mod.rs"),
        "        let incoming = self.bind_incoming(addr)?;",
        "        let incoming = self.bind_incoming(addr).await?;",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/channel/uds_connector.rs"),
        "#[cfg(not(target_os = \"windows\"))]\nuse tokio::net::UnixStream;\n\n#[cfg(not(target_os = \"windows\"))]",
        "#[cfg(not(any(target_os = \"windows\", target_os = \"trueos\", target_os = \"zkvm\")))]\nuse tokio::net::UnixStream;\n\n#[cfg(not(any(target_os = \"windows\", target_os = \"trueos\", target_os = \"zkvm\")))]",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/channel/uds_connector.rs"),
        "#[cfg(target_os = \"windows\")]\n#[allow(dead_code)]\ntype UnixStream = tokio::io::DuplexStream;\n\n#[cfg(target_os = \"windows\")]",
        "#[cfg(any(target_os = \"windows\", target_os = \"trueos\", target_os = \"zkvm\"))]\n#[allow(dead_code)]\ntype UnixStream = tokio::io::DuplexStream;\n\n#[cfg(any(target_os = \"windows\", target_os = \"trueos\", target_os = \"zkvm\"))]",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/channel/uds_connector.rs"),
        "\"uds connections are not allowed on windows\".into(),",
        "\"uds connections are not allowed on this platform\".into(),",
    )?;
    replace_file_text(
        &crate_dir.join("src/transport/channel/service/connection.rs"),
        "        let fut = self.inner.send_request(req);\n\n        Box::pin(async move { fut.await.map_err(Into::into).map(|res| res.map(Body::new)) })",
        "        let mut inner = self.inner.clone();\n        let fut = inner.send_request(req);\n\n        Box::pin(async move { fut.await.map_err(Into::into).map(|res| res.map(Body::new)) })",
    )?;
    Ok(())
}

fn patch_hyper_timeout_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        "use std::io;",
        "use hyper::io;",
    )?;
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        ".map_err(|e| io::Error::new(io::ErrorKind::TimedOut, e))?",
        ".map_err(|_| io::Error::new(io::ErrorKind::TimedOut, \"connection timed out\"))?",
    )?;
    replace_file_text(
        &crate_dir.join("src/stream.rs"),
        "use std::io;",
        "use hyper::io;",
    )
}

fn patch_tower_tokio_std_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        "#![cfg_attr(any(target_os = \"trueos\", target_os = \"zkvm\"), no_std)]",
        "",
    )?;
    replace_file_text(
        &crate_dir.join("src/buffer/worker.rs"),
        "use std::sync::{Arc, Mutex};",
        "use spin::Mutex;\nuse std::sync::Arc;",
    )?;
    replace_file_text(
        &crate_dir.join("src/buffer/worker.rs"),
        "let mut inner = self.handle.inner.lock().unwrap();",
        "let mut inner = self.handle.inner.lock();",
    )?;
    replace_file_text(
        &crate_dir.join("src/buffer/worker.rs"),
        ".lock()\n            .unwrap()\n            .as_ref()",
        ".lock()\n            .as_ref()",
    )?;
    replace_file_text(
        &crate_dir.join("src/load/peak_ewma.rs"),
        "sync::{Arc, Mutex}",
        "sync::Arc",
    )?;
    replace_file_text(
        &crate_dir.join("src/load/peak_ewma.rs"),
        "use tokio::time::Instant;",
        "use spin::Mutex;\nuse tokio::time::Instant;",
    )?;
    replace_file_text(
        &crate_dir.join("src/load/peak_ewma.rs"),
        "let mut rtt = self.rtt_estimate.lock().expect(\"peak ewma prior_estimate\");",
        "let mut rtt = self.rtt_estimate.lock();",
    )?;
    replace_file_text(
        &crate_dir.join("src/load/peak_ewma.rs"),
        "        if let Ok(mut rtt) = self.rtt_estimate.lock() {\n            rtt.update(self.sent_at, recv_at, self.decay_ns);\n        }",
        "        let mut rtt = self.rtt_estimate.lock();\n        rtt.update(self.sent_at, recv_at, self.decay_ns);",
    )?;
    replace_file_text(
        &crate_dir.join("src/load/peak_ewma.rs"),
        "    /// The default RTT estimate decays, so that new nodes are considered if the\n    /// default RTT is too high.",
        "    // The default RTT estimate decays, so that new nodes are considered if the\n    // default RTT is too high.",
    )?;
    append_if_missing(
        &crate_dir.join("Cargo.toml"),
        "[dependencies.spin]",
        "\n[dependencies.spin]\nversion = \"0.10.0\"\ndefault-features = false\nfeatures = [\"spin_mutex\"]\n",
    )
}

fn patch_ctrlc_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/platform/mod.rs"),
        "#[cfg(unix)]\nmod unix;\n\n#[cfg(windows)]",
        "#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\nmod unix;\n\n#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\nmod trueos;\n\n#[cfg(windows)]",
    )?;
    replace_file_text(
        &crate_dir.join("src/platform/mod.rs"),
        "#[cfg(unix)]\npub use self::unix::*;\n\n#[cfg(windows)]",
        "#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\npub use self::trueos::*;\n\n#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\npub use self::unix::*;\n\n#[cfg(windows)]",
    )?;
    replace_file_text(
        &crate_dir.join("Cargo.toml"),
        "[target.\"cfg(unix)\".dependencies.nix]",
        "[target.\"cfg(all(unix, not(any(target_os = \\\"trueos\\\", target_os = \\\"zkvm\\\"))))\".dependencies.nix]",
    )?;
    replace_file_text(
        &crate_dir.join("src/error.rs"),
        "#[cfg(not(windows))]\n        if e == platform::Error::EEXIST {",
        "#[cfg(all(not(windows), not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\n        if e == platform::Error::EEXIST {",
    )?;

    let trueos_dir = crate_dir.join("src/platform/trueos");
    fs::create_dir_all(&trueos_dir).map_err(io_string)?;
    fs::write(trueos_dir.join("mod.rs"), CTRL_C_TRUEOS_PLATFORM_RS).map_err(io_string)?;
    Ok(())
}

fn patch_argmax_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("Cargo.toml"),
        "[dependencies.nix]",
        "[target.\"cfg(all(unix, not(any(target_os = \\\"trueos\\\", target_os = \\\"zkvm\\\"))))\".dependencies.nix]",
    )?;
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        "#[cfg(not(unix))]\nmod other;\n#[cfg(unix)]\nmod unix;",
        "#[cfg(any(not(unix), target_os = \"trueos\", target_os = \"zkvm\"))]\nmod other;\n#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\nmod unix;",
    )?;
    replace_file_text(
        &crate_dir.join("src/lib.rs"),
        "#[cfg(not(unix))]\nuse other as platform;\n#[cfg(unix)]\nuse unix as platform;",
        "#[cfg(any(not(unix), target_os = \"trueos\", target_os = \"zkvm\"))]\nuse other as platform;\n#[cfg(all(unix, not(any(target_os = \"trueos\", target_os = \"zkvm\"))))]\nuse unix as platform;",
    )?;
    Ok(())
}

fn patch_rustix_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    let ioctl_mod = crate_dir.join("src/ioctl/mod.rs");
    let source = fs::read_to_string(&ioctl_mod).map_err(io_string)?;
    if source.contains("type _RawOpcode = c::c_int;") {
        replace_file_text(
            &ioctl_mod,
            "    target_os = \"wasi\",\n    target_os = \"nto\"\n))]\ntype _RawOpcode = c::c_int;",
            "    target_os = \"wasi\",\n    target_os = \"nto\"\n))]\ntype _RawOpcode = c::c_int;\n\n#[cfg(target_os = \"trueos\")]\ntype _RawOpcode = crate::backend::c::Ioctl;",
        )?;
    } else {
        replace_file_text(
            &ioctl_mod,
            "    target_os = \"nto\",\n    target_os = \"wasi\",\n))]\ntype _Opcode = c::c_int;",
            "    target_os = \"nto\",\n    target_os = \"wasi\",\n))]\ntype _Opcode = c::c_int;\n\n#[cfg(target_os = \"trueos\")]\ntype _Opcode = crate::backend::c::Ioctl;",
        )?;
    }

    let fallocate_types = crate_dir.join("src/backend/libc/fs/types.rs");
    replace_file_text(
        &fallocate_types,
        "            target_os = \"linux\",\n            target_os = \"wasi\",\n        )))]\n        const NO_HIDE_STALE",
        "            target_os = \"linux\",\n            target_os = \"trueos\",\n            target_os = \"wasi\",\n        )))]\n        const NO_HIDE_STALE",
    )
}

fn patch_socket2_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/sys/unix.rs"),
        "    target_os = \"cygwin\",\n))]\ntype IovLen = c_int;",
        "    target_os = \"cygwin\",\n    target_os = \"trueos\",\n    target_os = \"zkvm\",\n))]\ntype IovLen = c_int;",
    )
}

const CROSSTERM_TRUEOS_MIO_SOURCE: &str =
    include_str!("../vendor/crossterm-0.29.0-trueos/src/event/source/unix/mio.rs");

fn patch_crossterm_trueos_overlay(crate_dir: &Path) -> Result<(), String> {
    // Crossterm 0.28 and 0.29 use the same Mio event-source shape. Keep one
    // audited TRUEOS implementation instead of replaying a growing sequence of
    // text substitutions that can leave staged and vendored builds divergent.
    let mio_path = crate_dir.join("src/event/source/unix/mio.rs");
    normalize_line_endings(&mio_path)?;
    fs::write(&mio_path, CROSSTERM_TRUEOS_MIO_SOURCE).map_err(io_string)
}

fn normalize_line_endings(path: &Path) -> Result<(), String> {
    let original = fs::read_to_string(path).map_err(io_string)?;
    if original.contains("\r\n") {
        fs::write(path, original.replace("\r\n", "\n")).map_err(io_string)?;
    }
    Ok(())
}

fn patch_hyper_util_tokio_std_overlay(crate_dir: &Path) -> Result<(), String> {
    replace_file_text(
        &crate_dir.join("src/rt/tokio.rs"),
        "fn hyper_to_tokio_slices<'buf>(\n    bufs: &'buf [hyper_io::IoSlice<'buf>],\n) -> Vec<IoSlice<'buf>> {\n    bufs.iter().map(|buf| IoSlice::new(&**buf)).collect()\n}",
        "fn hyper_to_tokio_slices<'buf>(\n    bufs: &'buf [hyper_io::IoSlice<'buf>],\n) -> Vec<IoSlice<'buf>> {\n    bufs.iter().map(|buf| IoSlice::new(&**buf)).collect()\n}\n\n#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\nfn hyper_to_tokio_instant(deadline: Instant) -> tokio::time::Instant {\n    let hyper_now = Instant::now();\n    let tokio_now = tokio::time::Instant::now();\n    if deadline >= hyper_now {\n        tokio_now + deadline.duration_since(hyper_now)\n    } else {\n        tokio_now - hyper_now.duration_since(deadline)\n    }\n}\n\n#[cfg(not(any(target_os = \"trueos\", target_os = \"zkvm\")))]\nfn hyper_to_tokio_instant(deadline: Instant) -> tokio::time::Instant {\n    deadline.into()\n}\n\n#[cfg(any(target_os = \"trueos\", target_os = \"zkvm\"))]\nfn tokio_to_hyper_instant(instant: tokio::time::Instant) -> Instant {\n    let tokio_now = tokio::time::Instant::now();\n    let hyper_now = Instant::now();\n    if instant >= tokio_now {\n        hyper_now + (instant - tokio_now)\n    } else {\n        hyper_now - (tokio_now - instant)\n    }\n}\n\n#[cfg(not(any(target_os = \"trueos\", target_os = \"zkvm\")))]\nfn tokio_to_hyper_instant(instant: tokio::time::Instant) -> Instant {\n    instant.into()\n}",
    )?;
    replace_file_text(
        &crate_dir.join("src/rt/tokio.rs"),
        "inner: tokio::time::sleep_until(deadline.into()),",
        "inner: tokio::time::sleep_until(hyper_to_tokio_instant(deadline)),",
    )?;
    replace_file_text(
        &crate_dir.join("src/rt/tokio.rs"),
        "tokio::time::Instant::now().into()",
        "tokio_to_hyper_instant(tokio::time::Instant::now())",
    )?;
    replace_file_text(
        &crate_dir.join("src/rt/tokio.rs"),
        "self.project().inner.as_mut().reset(deadline.into());",
        "self.project().inner.as_mut().reset(hyper_to_tokio_instant(deadline));",
    )?;
    replace_file_text(
        &crate_dir.join("src/client/legacy/pool.rs"),
        "now.saturating_duration_since(entry.idle_at) > dur",
        "now.duration_since(entry.idle_at) > dur",
    )?;
    replace_file_text(
        &crate_dir.join("src/client/legacy/pool.rs"),
        "now.saturating_duration_since(instant) > timeout",
        "now.duration_since(instant) > timeout",
    )?;
    replace_file_text(
        &crate_dir.join("src/client/legacy/connect/http.rs"),
        "            socket.bind(&SocketAddr::new((*addr).into(), 0).into())?;",
        "            socket\n                .bind(&SocketAddr::new((*addr).into(), 0).into())\n                .map_err(|_| io::Error::new(io::ErrorKind::AddrNotAvailable, \"socket bind failed\"))?;",
    )?;
    replace_file_text(
        &crate_dir.join("src/client/legacy/connect/http.rs"),
        "                socket.bind(&any.into())?;",
        "                socket\n                    .bind(&any.into())\n                    .map_err(|_| io::Error::new(io::ErrorKind::AddrNotAvailable, \"socket bind failed\"))?;",
    )?;
    Ok(())
}

fn replace_file_text(path: &Path, needle: &str, replacement: &str) -> Result<(), String> {
    let original = fs::read_to_string(path).map_err(io_string)?;
    if !original.contains(needle) {
        return Err(format!(
            "failed to patch {}; missing expected text",
            path.display()
        ));
    }
    let rewritten = original.replace(needle, replacement);
    fs::write(path, rewritten).map_err(io_string)
}

fn append_if_missing(path: &Path, needle: &str, addition: &str) -> Result<(), String> {
    let mut original = fs::read_to_string(path).map_err(io_string)?;
    if original.contains(needle) {
        return Ok(());
    }
    original.push_str(addition);
    fs::write(path, original).map_err(io_string)
}

const FUTURES_TIMER_TRUEOS_RS: &str = r#"//! TRUEOS timer backend: use the application's existing Tokio timer wheel.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use trueos_tokio::time::{Instant, Sleep, sleep_until};

/// A future which becomes ready after a duration has elapsed.
///
/// On TRUEOS this is deliberately a Tokio timer entry, not a helper pthread.
/// The sleep is created lazily so constructing a libp2p behaviour immediately
/// before entering its runtime remains valid.
pub struct Delay {
    deadline: Instant,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl Delay {
    /// Creates a new delay.
    #[inline]
    pub fn new(duration: Duration) -> Self {
        Self {
            deadline: deadline_after(duration),
            sleep: None,
        }
    }

    /// Restarts this delay with a new duration.
    #[inline]
    pub fn reset(&mut self, duration: Duration) {
        self.deadline = deadline_after(duration);
        if let Some(sleep) = self.sleep.as_mut() {
            sleep.as_mut().reset(self.deadline);
        }
    }
}

impl Future for Delay {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.sleep.is_none() {
            this.sleep = Some(Box::pin(sleep_until(this.deadline)));
        }
        this.sleep.as_mut().expect("sleep initialized").as_mut().poll(cx)
    }
}

impl fmt::Debug for Delay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delay")
            .field("deadline", &self.deadline)
            .finish()
    }
}

fn deadline_after(duration: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(duration)
        .unwrap_or_else(|| now + Duration::from_secs(86_400 * 365 * 30))
}
"#;

const CTRL_C_TRUEOS_PLATFORM_RS: &str = r#"// Patched by trueos-blueprint during SDK staging.

use std::io;
use std::thread;

/// Platform specific error type.
pub type Error = io::Error;

/// Platform specific signal type.
pub type Signal = u32;

/// Register os signal handler.
#[inline]
pub unsafe fn init_os_handler(_overwrite: bool) -> Result<(), Error> {
    Ok(())
}

/// Blocks until a Ctrl-C signal is received.
#[inline]
pub unsafe fn block_ctrl_c() -> Result<(), Error> {
    loop {
        thread::park();
    }
}
"#;

fn staged_manifest_for_overlay(
    app_dir: &Path,
    manifest_path: &Path,
    work_dir: &Path,
    build_settings: &BuildSettings,
    source_overlay: &[CratePatch],
    lock_mismatches: &[LockMismatch],
) -> Result<Option<PathBuf>, String> {
    let stage_for_source_rewrite =
        staged_source_needs_trueos_collection_rewrite(app_dir, manifest_path)?;
    if source_overlay.is_empty()
        && !build_settings.shims.add_no_std
        && !build_settings.shims.add_entrypoint
        && !stage_for_source_rewrite
    {
        return Ok(None);
    }

    let staged_app_dir = work_dir.join("source-overlay-app");
    copy_app_tree(app_dir, &staged_app_dir)?;
    link_blueprint_siblings_for_staged_app(app_dir, work_dir)?;
    let manifest_relative = manifest_path.strip_prefix(app_dir).map_err(|_| {
        format!(
            "manifest path {} is not under app dir {}",
            manifest_path.display(),
            app_dir.display()
        )
    })?;
    let staged_manifest = staged_app_dir.join(manifest_relative);
    let nested_workspace_package = manifest_relative.components().count() > 1;
    canonicalize_staged_manifest_paths_from_original(manifest_path, &staged_manifest)?;
    isolate_staged_workspace_members(&staged_manifest)?;
    strip_manifest_patch_section(&staged_manifest)?;
    if !nested_workspace_package {
        materialize_staged_workspace_dependencies(
            app_dir,
            work_dir,
            &staged_manifest,
            source_overlay,
        )?;
    }
    materialize_trueos_blueprint_dependency(app_dir, work_dir, &staged_manifest)?;
    materialize_hidden_build_std_pins(&staged_manifest, build_settings, source_overlay)?;
    if !nested_workspace_package {
        ensure_standalone_manifest_workspace(&staged_manifest)?;
    }
    canonicalize_staged_blueprint_dependency_paths(app_dir, &staged_app_dir)?;
    rewrite_staged_source_for_target(app_dir, &staged_app_dir, &staged_manifest, build_settings)?;
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

/// Staging copies only the application tree. Resolve dependency paths against
/// the original manifest before that move so sibling engine workspaces remain
/// the same Cargo package identities instead of accidentally pointing under
/// `target/trueos-blueprint`.
fn canonicalize_staged_manifest_paths_from_original(
    original_manifest: &Path,
    staged_manifest: &Path,
) -> Result<(), String> {
    let original_dir = original_manifest.parent().unwrap_or(original_manifest);
    let cargo_toml = fs::read_to_string(staged_manifest).map_err(io_string)?;
    let mut changed = false;
    let mut out = String::with_capacity(cargo_toml.len());
    for line in cargo_toml.lines() {
        let rewritten = inline_dependency_name_and_path(line).and_then(|(dependency, path)| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                return None;
            }
            fs::canonicalize(original_dir.join(path))
                .ok()
                .and_then(|canonical| dependency_with_rewritten_path(line, dependency, &canonical))
        });
        let output_line = rewritten.as_deref().unwrap_or(line);
        changed |= output_line != line;
        out.push_str(output_line);
        out.push('\n');
    }
    if changed {
        fs::write(staged_manifest, out).map_err(io_string)?;
    }
    Ok(())
}

/// Build only the selected package from a staged application workspace.
///
/// Direct path dependencies are rewritten to their canonical source paths
/// above. Keeping copied workspace members as well would give Cargo two path
/// identities for the same name and version: one below the staging directory
/// and one at the canonical source path. That cannot be represented in a
/// lockfile unambiguously.
fn isolate_staged_workspace_members(manifest_path: &Path) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut out = String::with_capacity(cargo_toml.len());
    let mut in_workspace = false;
    let mut skipping_array = false;
    let mut array_depth = 0i32;

    for line in cargo_toml.lines() {
        if skipping_array {
            array_depth += toml_array_bracket_delta(line);
            if array_depth <= 0 {
                skipping_array = false;
            }
            continue;
        }

        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_workspace = trimmed == "[workspace]";
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let key = key.trim();
        if !in_workspace || !matches!(key, "members" | "default-members") {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let indent_len = line.len() - line.trim_start().len();
        out.push_str(&line[..indent_len]);
        out.push_str(key);
        out.push_str(" = [\".\"]\n");

        array_depth = toml_array_bracket_delta(value);
        skipping_array = array_depth > 0;
    }

    fs::write(manifest_path, out).map_err(io_string)
}

fn toml_array_bracket_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted || character == '#' {
            if character == '#' && !quoted {
                break;
            }
            continue;
        }
        match character {
            '[' => delta += 1,
            ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn rewrite_staged_source_for_target(
    app_dir: &Path,
    staged_app_dir: &Path,
    staged_manifest: &Path,
    build_settings: &BuildSettings,
) -> Result<(), String> {
    let rewrite_collections = manifest_has_dependency(staged_manifest, "trueos")?
        && manifest_trueos_rewrite_std_imports(staged_manifest)?;
    if rewrite_collections {
        rewrite_trueos_collection_imports(staged_app_dir)?;
    }

    if !build_settings.shims.add_no_std && !build_settings.shims.add_entrypoint {
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
    if build_settings.shims.add_no_std {
        header.push_str("#![no_std]\n");
    }
    if build_settings.shims.add_entrypoint {
        header.push_str("#![no_main]\n");
    }

    let mut rewritten = String::with_capacity(original.len() + header.len() + 128);
    rewritten.push_str(&header);
    rewritten.push_str(&original);
    if !original.ends_with('\n') {
        rewritten.push('\n');
    }
    if build_settings.shims.add_entrypoint {
        if build_settings.shims.add_no_std {
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

fn staged_source_needs_trueos_collection_rewrite(
    app_dir: &Path,
    manifest_path: &Path,
) -> Result<bool, String> {
    if !manifest_trueos_rewrite_std_imports(manifest_path)? {
        return Ok(false);
    }

    if !manifest_has_dependency(manifest_path, "trueos")?
        && !manifest_declared_features(manifest_path)?
            .iter()
            .any(|feature| feature == "trueos-blueprint")
    {
        return Ok(false);
    }

    for needle in [
        "std::collections::HashMap",
        "std::collections::HashSet",
        "std::collections::BTreeMap",
        "std::collections::BTreeSet",
        "use std::collections::{",
        "std::time::Instant",
        "use std::time::{",
    ] {
        if source_tree_mentions(app_dir, needle)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn manifest_trueos_rewrite_std_imports(manifest_path: &Path) -> Result<bool, String> {
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
        if key.trim() != "rewrite-std-imports" {
            continue;
        }
        return match toml_bool_value(value.trim()) {
            Some(value) => Ok(value),
            None => Err(format!(
                "bad trueos-blueprint rewrite-std-imports in {}",
                manifest_path.display()
            )),
        };
    }
    Ok(true)
}

fn rewrite_trueos_collection_imports(staged_app_dir: &Path) -> Result<(), String> {
    let source_dir = staged_app_dir.join("src");
    if !source_dir.is_dir() {
        return Ok(());
    }

    let mut changed_files = 0usize;
    rewrite_trueos_collection_imports_in_dir(&source_dir, &mut changed_files)?;
    if changed_files > 0 {
        println!(
            "trueos-blueprint: rewrote std collection imports for trueos: {changed_files} files"
        );
    }
    Ok(())
}

fn rewrite_trueos_collection_imports_in_dir(
    dir: &Path,
    changed_files: &mut usize,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_string)?;
        if file_type.is_dir() {
            rewrite_trueos_collection_imports_in_dir(&path, changed_files)?;
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        let source = fs::read_to_string(&path).map_err(io_string)?;
        let rewritten = rewrite_trueos_collection_imports_in_source(&source);
        if rewritten != source {
            fs::write(&path, rewritten).map_err(io_string)?;
            *changed_files += 1;
        }
    }
    Ok(())
}

fn rewrite_trueos_collection_imports_in_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        out.push_str(&rewrite_trueos_collection_import_line(line));
        out.push('\n');
    }
    if !source.ends_with('\n') {
        out.pop();
    }
    out
}

fn rewrite_trueos_collection_import_line(line: &str) -> String {
    let line = rewrite_trueos_time_import_line(line);
    if let Some(rewritten) = rewrite_trueos_grouped_collection_import(&line) {
        return rewritten;
    }

    let mut rewritten = line;
    for (from, to) in [
        ("std::collections::HashMap", "trueos::collections::HashMap"),
        ("std::collections::HashSet", "trueos::collections::HashSet"),
        (
            "std::collections::BTreeMap",
            "trueos::collections::BTreeMap",
        ),
        (
            "std::collections::BTreeSet",
            "trueos::collections::BTreeSet",
        ),
    ] {
        rewritten = rewritten.replace(from, to);
    }
    rewritten
}

fn rewrite_trueos_time_import_line(line: &str) -> String {
    if let Some(rewritten) = rewrite_trueos_grouped_time_import(line) {
        return rewritten;
    }

    line.replace("std::time::Instant", "trueos::clock::Instant")
}

fn rewrite_trueos_grouped_time_import(line: &str) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let trimmed = line.trim_start();
    let inner = trimmed
        .strip_prefix("use std::time::{")?
        .strip_suffix("};")?;
    let mut std_items = Vec::new();
    let mut has_instant = false;
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item == "Instant" {
            has_instant = true;
        } else {
            std_items.push(item);
        }
    }
    if !has_instant {
        return None;
    }

    let mut out = String::new();
    if !std_items.is_empty() {
        out.push_str(&format!(
            "{indent}use std::time::{{{}}};\n",
            std_items.join(", ")
        ));
    }
    out.push_str(&format!("{indent}use trueos::clock::Instant;"));
    Some(out)
}

fn rewrite_trueos_grouped_collection_import(line: &str) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let trimmed = line.trim_start();
    let inner = trimmed
        .strip_prefix("use std::collections::{")?
        .strip_suffix("};")?;
    let mut items = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if !matches!(item, "BTreeMap" | "BTreeSet" | "HashMap" | "HashSet") {
            return None;
        }
        items.push(item);
    }
    if items.is_empty() {
        return None;
    }

    Some(format!(
        "{indent}use trueos::collections::{{{}}};",
        items.join(", ")
    ))
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
    root: Option<String>,
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
    #[serde(default)]
    dep_kinds: Vec<MetadataDepKind>,
}

#[derive(Deserialize)]
struct MetadataDepKind {
    kind: Option<String>,
}

#[derive(Debug, Default)]
struct RustcPayloadSelection {
    direct_externs: Vec<RustcPayloadDirectExtern>,
    artifacts: Vec<cargo_output::CargoTargetMetadataArtifact>,
}

#[derive(Debug)]
struct RustcPayloadDirectExtern {
    alias: String,
    crate_name: String,
    path: PathBuf,
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
        let target_key = (mismatch.name.clone(), mismatch.locked_version.clone());
        match overlay_targets.entry(target_key) {
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
                    "overlay version for {} {} is inconsistent: {} vs {}",
                    mismatch.name,
                    mismatch.locked_version,
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
            let target_key = (dep_package.name.clone(), dep_package.version.clone());
            let Some(target) = overlay_targets.get(&target_key) else {
                continue;
            };
            if dep_package.version == target.overlay_version {
                continue;
            }

            let Some(declared_dependency) =
                declared_dependency_for_resolved_edge(&parent_package.dependencies, &dep.name)
            else {
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
    let mut metadata = toolchain::cargo_command();
    if let Some(manifest_dir) = manifest_path.parent() {
        metadata.current_dir(manifest_dir);
    } else {
        metadata.current_dir(app_dir);
    }
    metadata
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
            return Err(format!(
                "cargo metadata failed with status {}",
                output.status
            ));
        }
        return Err(format!("cargo metadata failed: {stderr}"));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse cargo metadata JSON: {err}"))
}

fn cargo_metadata_for_rustc_payload(
    manifest_path: &Path,
    target_spec: &Path,
    source_overlay: &[CratePatch],
    no_default_features: bool,
    features: &[String],
) -> Result<CargoMetadata, String> {
    let mut metadata = toolchain::cargo_command();
    if let Some(manifest_dir) = manifest_path.parent() {
        metadata.current_dir(manifest_dir);
    }
    metadata
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--locked")
        .arg("-Z")
        .arg("json-target-spec")
        .arg("--filter-platform")
        .arg(target_spec)
        .arg("--manifest-path")
        .arg(manifest_path);
    push_source_overlay_configs(&mut metadata, source_overlay);
    if no_default_features {
        metadata.arg("--no-default-features");
    }
    if !features.is_empty() {
        metadata.arg("--features").arg(features.join(","));
    }
    metadata.env("RUSTC_BOOTSTRAP_SYNTHETIC_TARGET", "1");

    let output = metadata
        .output()
        .map_err(|err| format!("rustc payload cargo metadata failed to start: {err}"))?;
    let notes = write_filtered_cargo_output("rustc payload cargo metadata", &[], &output.stderr)?;
    print_cargo_output_notes("rustc payload cargo metadata", &notes);
    if !output.status.success() {
        return Err(format!(
            "rustc payload cargo metadata failed with status {}",
            output.status
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse rustc payload cargo metadata JSON: {err}"))
}

fn metadata_dependency_is_normal(dependency: &MetadataNodeDep) -> bool {
    dependency.dep_kinds.is_empty() || dependency.dep_kinds.iter().any(|kind| kind.kind.is_none())
}

fn cargo_extern_name(name: &str) -> String {
    name.replace('-', "_")
}

fn select_rustc_payload(
    metadata: &CargoMetadata,
    requested_roots: &[String],
    target_artifacts: &[cargo_output::CargoTargetMetadataArtifact],
) -> Result<RustcPayloadSelection, String> {
    if requested_roots.is_empty() {
        return Ok(RustcPayloadSelection::default());
    }

    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| "cargo metadata returned no resolve graph for rustc payload".to_owned())?;
    let root_id = resolve
        .root
        .as_deref()
        .ok_or_else(|| "cargo metadata returned no root package for rustc payload".to_owned())?;
    let nodes = resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let root = nodes
        .get(root_id)
        .copied()
        .ok_or_else(|| format!("cargo metadata omitted root node {root_id}"))?;

    let mut direct_packages = Vec::with_capacity(requested_roots.len());
    for requested in requested_roots {
        let alias = cargo_extern_name(requested);
        let mut matches = root.deps.iter().filter(|dependency| {
            metadata_dependency_is_normal(dependency)
                && cargo_extern_name(&dependency.name) == alias
        });
        let Some(dependency) = matches.next() else {
            return Err(format!(
                "rustc payload dependency `{requested}` is not an active normal dependency \
                 of Cargo root {root_id}"
            ));
        };
        if matches.any(|candidate| candidate.pkg != dependency.pkg) {
            return Err(format!(
                "rustc payload dependency alias `{requested}` resolves ambiguously from \
                 Cargo root {root_id}"
            ));
        }
        direct_packages.push((alias, dependency.pkg.as_str()));
    }

    let mut reachable = BTreeSet::new();
    let mut pending = direct_packages
        .iter()
        .map(|(_, package_id)| *package_id)
        .collect::<Vec<_>>();
    while let Some(package_id) = pending.pop() {
        if !reachable.insert(package_id) {
            continue;
        }
        let Some(node) = nodes.get(package_id).copied() else {
            continue;
        };
        pending.extend(
            node.deps
                .iter()
                .filter(|dependency| metadata_dependency_is_normal(dependency))
                .map(|dependency| dependency.pkg.as_str()),
        );
    }

    let artifacts = target_artifacts
        .iter()
        .filter(|artifact| reachable.contains(artifact.package_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut direct_externs = Vec::with_capacity(direct_packages.len());
    for (alias, package_id) in direct_packages {
        let mut matches = artifacts
            .iter()
            .filter(|artifact| artifact.package_id == package_id);
        let Some(artifact) = matches.next() else {
            return Err(format!(
                "rustc payload dependency `{alias}` ({package_id}) produced no target rmeta; \
                 direct proc-macro and host-only dependencies are unsupported"
            ));
        };
        if matches.next().is_some() {
            return Err(format!(
                "rustc payload dependency `{alias}` ({package_id}) produced multiple target \
                 rmeta artifacts; refusing to guess its library target"
            ));
        }
        direct_externs.push(RustcPayloadDirectExtern {
            alias,
            crate_name: artifact.crate_name.clone(),
            path: artifact.path.clone(),
        });
    }

    for direct in &direct_externs {
        if direct.alias != direct.crate_name
            && artifacts
                .iter()
                .any(|artifact| artifact.crate_name == direct.alias && artifact.path != direct.path)
        {
            return Err(format!(
                "rustc payload alias `{}` collides with a different closure crate name; \
                 exact extern lookup would be ambiguous",
                direct.alias
            ));
        }
    }

    Ok(RustcPayloadSelection {
        direct_externs,
        artifacts,
    })
}

fn dependency_display_name(dependency: &MetadataDependency) -> &str {
    dependency.rename.as_deref().unwrap_or(&dependency.name)
}

fn declared_dependency_for_resolved_edge<'a>(
    dependencies: &'a [MetadataDependency],
    resolved_name: &str,
) -> Option<&'a MetadataDependency> {
    dependencies
        .iter()
        .find(|dependency| dependency_display_name(dependency).replace('-', "_") == resolved_name)
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
        return Ok(exact_req_matches(
            version,
            &parse_req_version(rest.trim(), token)?,
        ));
    }
    if let Some(rest) = token.strip_prefix('^') {
        return Ok(caret_req_matches(
            version,
            &parse_req_version(rest.trim(), token)?,
        ));
    }
    if let Some(rest) = token.strip_prefix('~') {
        return Ok(tilde_req_matches(
            version,
            &parse_req_version(rest.trim(), token)?,
        ));
    }
    if token.contains('*') || token.contains('x') || token.contains('X') {
        return Ok(wildcard_req_matches(version, &parse_req_prefix(token)?));
    }
    Ok(caret_req_matches(
        version,
        &parse_req_version(token, token)?,
    ))
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

fn link_blueprint_siblings_for_staged_app(app_dir: &Path, work_dir: &Path) -> Result<(), String> {
    let blueprint_root = blueprint_root(app_dir).unwrap_or_else(|| app_dir.to_path_buf());
    let staging_root = work_dir.parent().unwrap_or(work_dir);
    let app_target_root = app_dir.join("target");

    link_staged_sibling(&staging_root.join("api"), &blueprint_root.join("api"))?;
    link_staged_sibling(&staging_root.join("vendor"), &blueprint_root.join("vendor"))?;
    link_staged_sibling(&staging_root.join("crates"), &blueprint_root.join("crates"))?;
    link_staged_sibling(
        &app_target_root.join("crates"),
        &blueprint_root.join("crates"),
    )?;

    if let Some(kernel_manifest) = trueos_kernel_manifest(app_dir)
        && let Some(kernel_root) = kernel_manifest.parent()
    {
        link_staged_sibling(&staging_root.join("TRUEOS"), kernel_root)?;
    }

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
        if patch.key != patch.name {
            cmd.arg("--config").arg(format!(
                "patch.crates-io.{}.package={}",
                patch.key,
                toml_string(&patch.name)
            ));
        }
        cmd.arg("--config").arg(format!(
            "patch.crates-io.{}.path={}",
            patch.key,
            toml_string(&patch.path.to_string_lossy())
        ));
    }
}

fn staged_source_overlay(source_overlay: &[CratePatch], _work_dir: &Path) -> Vec<CratePatch> {
    source_overlay
        .iter()
        .map(|patch| CratePatch {
            key: patch.key.clone(),
            name: patch.name.clone(),
            // Preserve the canonical vendor identity. A symlinked overlay may
            // contain relative Blueprint-crate dependencies; mixing those with
            // the canonical SDK produces duplicate path package IDs.
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

    let blueprint_root = blueprint_root(app_dir)?;
    let candidate = blueprint_root.parent()?.join("TRUEOS").join("Cargo.toml");
    if candidate.is_file() && package_name(&candidate).ok().as_deref() == Some("TRUEOS") {
        return Some(candidate);
    }

    None
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
    source_overlay: &[CratePatch],
) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let blueprint_root = blueprint_root(app_dir).unwrap_or_else(|| app_dir.to_path_buf());
    let mut changed = false;
    let mut out = String::with_capacity(cargo_toml.len());

    for line in cargo_toml.lines() {
        if let Some(dep_name) = workspace_dependency_name(line) {
            let mut dependency = materialized_workspace_dependency(
                app_dir,
                &blueprint_root,
                work_dir,
                source_overlay,
                &dep_name,
            )?;
            if workspace_dependency_is_optional(line) {
                dependency = optional_dependency_line(&dependency);
            }
            out.push_str(&dependency);
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

    let dependency = format!(
        "# Pinned because Rust build-std pulls libc outside the app dependency graph.\nlibc = {{ version = \"={libc_version}\", default-features = false }}"
    );
    insert_manifest_dependency(manifest_path, &dependency)
}

fn materialize_trueos_blueprint_dependency(
    app_dir: &Path,
    _work_dir: &Path,
    manifest_path: &Path,
) -> Result<(), String> {
    if !manifest_declared_features(manifest_path)?
        .iter()
        .any(|feature| feature == "trueos-blueprint")
        || manifest_has_dependency(manifest_path, "trueos")?
    {
        return Ok(());
    }

    let blueprint_root = blueprint_root(app_dir).unwrap_or_else(|| app_dir.to_path_buf());
    // Inject the canonical SDK path for external workspace roots. Routing this
    // edge through the staging symlink makes Cargo treat transitive sibling
    // paths (for example `crates/trueos-math`) as both their canonical and
    // symlinked package IDs, which cannot be represented in one lockfile.
    let trueos_api = blueprint_root.join("api");
    let dependency = format!(
        "# Injected by trueos-blueprint for external app packaging.\n{}",
        path_dependency_line("trueos", &trueos_api)
    );
    insert_manifest_dependency(manifest_path, &dependency)
}

fn canonicalize_staged_blueprint_dependency_paths(
    app_dir: &Path,
    staged_app_dir: &Path,
) -> Result<(), String> {
    let blueprint_root = blueprint_root(app_dir).unwrap_or_else(|| app_dir.to_path_buf());
    let canonical_blueprint_root =
        fs::canonicalize(&blueprint_root).unwrap_or_else(|_| blueprint_root.clone());
    canonicalize_staged_blueprint_dependency_paths_in_dir(staged_app_dir, &canonical_blueprint_root)
}

fn canonicalize_staged_blueprint_dependency_paths_in_dir(
    dir: &Path,
    canonical_blueprint_root: &Path,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_string)?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if !matches!(name.to_str(), Some(".git" | "target" | "dist")) {
                canonicalize_staged_blueprint_dependency_paths_in_dir(
                    &path,
                    canonical_blueprint_root,
                )?;
            }
        } else if file_type.is_file() && entry.file_name() == "Cargo.toml" {
            canonicalize_staged_blueprint_manifest_dependency_paths(
                &path,
                canonical_blueprint_root,
            )?;
        }
    }
    Ok(())
}

fn canonicalize_staged_blueprint_manifest_dependency_paths(
    manifest_path: &Path,
    canonical_blueprint_root: &Path,
) -> Result<(), String> {
    let manifest_dir = manifest_path.parent().unwrap_or(manifest_path);
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut changed = false;
    let mut out = String::with_capacity(cargo_toml.len());

    for line in cargo_toml.lines() {
        let mut rewritten = None;
        if let Some((dependency, dependency_path)) = inline_dependency_name_and_path(line) {
            let dependency_path = PathBuf::from(dependency_path);
            let staged_path = if dependency_path.is_absolute() {
                dependency_path
            } else {
                manifest_dir.join(dependency_path)
            };
            if let Ok(canonical_path) = fs::canonicalize(staged_path)
                && canonical_path.starts_with(&canonical_blueprint_root)
            {
                rewritten = dependency_with_rewritten_path(line, dependency, &canonical_path);
            }
        }
        let output_line = rewritten.as_deref().unwrap_or(line);
        changed |= output_line != line;
        out.push_str(output_line);
        out.push('\n');
    }

    if changed {
        fs::write(manifest_path, out).map_err(io_string)?;
    }
    Ok(())
}

fn inline_dependency_name_and_path(line: &str) -> Option<(&str, String)> {
    let declaration = line
        .split_once('#')
        .map_or(line, |(declaration, _)| declaration);
    let (key, value) = declaration.split_once('=')?;
    let dependency = key.trim();
    let path = inline_table_path(value.trim())?;
    Some((dependency, path))
}

fn dependency_with_rewritten_path(line: &str, dep_name: &str, path: &Path) -> Option<String> {
    let (declaration, comment) = line.split_once('#').unwrap_or((line, ""));
    let (key, value) = declaration.split_once('=')?;
    if key.trim() != dep_name {
        return None;
    }

    let old_path = inline_table_path(value.trim())?;
    let old_path = toml_string(old_path.as_str());
    let offset = value.find(old_path.as_str())?;
    let mut rewritten = String::with_capacity(line.len().saturating_add(path.as_os_str().len()));
    rewritten.push_str(key);
    rewritten.push('=');
    rewritten.push_str(&value[..offset]);
    rewritten.push_str(toml_string(path.to_string_lossy().as_ref()).as_str());
    rewritten.push_str(&value[offset + old_path.len()..]);
    if !comment.is_empty() {
        rewritten.push('#');
        rewritten.push_str(comment);
    }
    Some(rewritten)
}

fn insert_manifest_dependency(manifest_path: &Path, dependency: &str) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut out = String::with_capacity(cargo_toml.len() + dependency.len() + 2);
    let mut in_dependencies = false;
    let mut inserted = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if in_dependencies && trimmed.starts_with('[') {
            out.push_str(dependency);
            out.push('\n');
            inserted = true;
            in_dependencies = false;
        }

        out.push_str(line);
        out.push('\n');

        if trimmed == "[dependencies]" {
            in_dependencies = true;
        }
    }

    if !inserted {
        if !in_dependencies {
            out.push_str("\n[dependencies]\n");
        }
        out.push_str(dependency);
        out.push('\n');
    }

    fs::write(manifest_path, out).map_err(io_string)
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

fn workspace_dependency_is_optional(line: &str) -> bool {
    let line = line.split('#').next().unwrap_or("").trim();
    line.contains("optional") && line.contains("true")
}

fn optional_dependency_line(line: &str) -> String {
    if line.contains("optional") {
        return line.to_string();
    }
    let Some(index) = line.rfind('}') else {
        return line.to_string();
    };
    let mut out = String::with_capacity(line.len() + ", optional = true".len());
    out.push_str(&line[..index]);
    out.push_str(", optional = true");
    out.push_str(&line[index..]);
    out
}

fn materialized_workspace_dependency(
    app_dir: &Path,
    blueprint_root: &Path,
    work_dir: &Path,
    source_overlay: &[CratePatch],
    dep_name: &str,
) -> Result<String, String> {
    let line = match dep_name {
        "anyhow" => "anyhow = { version = \"1.0\", default-features = false }".to_string(),
        "axum" => format!(
            "axum = {{ path = {}, default-features = false, features = [\"http1\", \"json\", \"tokio\", \"ws\"] }}",
            toml_string(
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    "axum",
                    &blueprint_root.join("vendor/axum-0.8.9"),
                )
                .display()
                .to_string(),
            )
        ),
        "colored" => "colored = \"2.1\"".to_string(),
        "glob" => "glob = \"0.3\"".to_string(),
        "http-body-util" => {
            path_dependency_line(
                dep_name,
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    dep_name,
                    &blueprint_root.join("vendor/http-body-util-0.1.3"),
                ),
            )
        }
        "hyper" => format!(
            "hyper = {{ path = {}, default-features = false, features = [\"client\", \"server\", \"http1\", \"std\"] }}",
            toml_string(
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    "hyper",
                    &blueprint_root.join("vendor/hyper-1.9.0"),
                )
                .display()
                .to_string(),
            )
        ),
        "hyper-util" => {
            format!(
                "hyper-util = {{ path = {}, default-features = false, features = [\"std\", \"tokio\"] }}",
                toml_string(
                    &workspace_dependency_vendor_path(
                        source_overlay,
                        work_dir,
                        "hyper-util",
                        &blueprint_root.join("vendor/hyper-util-0.1.20"),
                    )
                    .display()
                    .to_string(),
                )
            )
        }
        "ignore" => "ignore = \"0.4\"".to_string(),
        "libm" => "libm = { version = \"0.2\", default-features = false }".to_string(),
        "regex" => {
            "regex = { version = \"1\", default-features = false, features = [\"perf\"] }"
                .to_string()
        }
        "reqwest" => format!(
            "reqwest = {{ path = {}, default-features = false, features = [\"json\", \"rustls\"] }}",
            toml_string(
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    "reqwest",
                    &blueprint_root.join("vendor/reqwest-0.13.3"),
                )
                .display()
                .to_string(),
            )
        ),
        "rustls" => {
            "rustls = { version = \"0.23.27\", default-features = false, features = [\"std\", \"tls12\"] }"
                .to_string()
        }
        "rustls-rustcrypto" => {
            format!(
                "rustls-rustcrypto = {{ path = {}, default-features = false, features = [\"std\", \"tls12\"] }}",
                toml_string(
                    &workspace_dependency_vendor_path(
                        source_overlay,
                        work_dir,
                        "rustls-rustcrypto",
                        &blueprint_root.join("vendor/rustls-rustcrypto-0.0.2-alpha"),
                    )
                    .display()
                    .to_string(),
                )
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
        "tokio" => format!(
            "tokio = {{ path = {}, version = \"=1.52.3\", default-features = false, features = [\"full\"] }}",
            toml_string(
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    "tokio",
                    &blueprint_root.join("vendor/tokio-1.52.3"),
                )
                .display()
                .to_string(),
            )
        ),
        "tokio-rustls" => {
            format!(
                "tokio-rustls = {{ path = {}, default-features = false, features = [\"tls12\"] }}",
                toml_string(
                    &workspace_dependency_vendor_path(
                        source_overlay,
                        work_dir,
                        "tokio-rustls",
                        &blueprint_root.join("vendor/tokio-rustls-0.26.4"),
                    )
                    .display()
                    .to_string(),
                )
            )
        }
        "tower" => format!(
            "tower = {{ path = {}, default-features = false, features = [\"util\"] }}",
            toml_string(
                &workspace_dependency_vendor_path(
                    source_overlay,
                    work_dir,
                    "tower",
                    &blueprint_root.join("vendor/tower-0.5.3"),
                )
                .display()
                .to_string(),
            )
        ),
        // Keep the SDK and all of its relative `../crates/*` dependencies on
        // one canonical path identity. Cargo does not canonicalize a staging
        // symlink consistently when it writes package IDs to Cargo.lock.
        "trueos" => path_dependency_line(dep_name, &blueprint_root.join("api")),
        "trueos-chat" => path_dependency_line(dep_name, &blueprint_root.join("apps/chatserver/trueos-chat")),
        "trueos-currency" => {
            path_dependency_line(dep_name, &blueprint_root.join("../uiout/currency_reqwest/trueos-currency"))
        }
        "trueos-flags" => {
            path_dependency_line(dep_name, &blueprint_root.join("../uiout/flags/trueos-flags"))
        }
        "trueos-gfx-core" => format!(
            "trueos-gfx-core = {{ path = {}, features = [\"alloc\"] }}",
            toml_string(
                &staged_blueprint_path(blueprint_root, work_dir, "crates/trueos-gfx-core")
                    .display()
                    .to_string(),
            )
        ),
        "trueos-tetris" => path_dependency_line(
            dep_name,
            &stage_trueos_tetris_crate(blueprint_root, work_dir)?,
        ),
        "trueos-weather" => {
            path_dependency_line(dep_name, &blueprint_root.join("../uiout/weather/trueos-weather"))
        }
        "webpki-roots" => {
            "webpki-roots = { version = \"1\", default-features = false }".to_string()
        }
        other => {
            return app_workspace_dependency_line(app_dir, other)?.ok_or_else(|| {
                format!(
                    "unsupported workspace dependency `{other}` in {}",
                    blueprint_root.display()
                )
            });
        }
    };
    Ok(line)
}

fn workspace_dependency_vendor_path(
    source_overlay: &[CratePatch],
    _work_dir: &Path,
    dep_name: &str,
    fallback: &Path,
) -> PathBuf {
    // Keep direct workspace dependencies on the same canonical path used by
    // `[patch.crates-io]`. Cargo treats a staging symlink and its real target as
    // different package IDs, which makes same-name/version packages collide in
    // the generated lockfile.
    source_overlay
        .iter()
        .find(|patch| patch.key == dep_name && patch.name == dep_name)
        .map(|patch| patch.path.clone())
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn staged_blueprint_path(blueprint_root: &Path, work_dir: &Path, relative: &str) -> PathBuf {
    let staged = work_dir.parent().unwrap_or(work_dir).join(relative);
    if staged.exists() {
        staged
    } else {
        blueprint_root.join(relative)
    }
}

fn app_workspace_dependency_line(app_dir: &Path, dep_name: &str) -> Result<Option<String>, String> {
    let Some((workspace_root, workspace_manifest)) = app_workspace_manifest(app_dir) else {
        return Ok(None);
    };
    let cargo_toml = fs::read_to_string(&workspace_manifest).map_err(io_string)?;
    let mut in_workspace_dependencies = false;

    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_workspace_dependencies = trimmed == "[workspace.dependencies]";
            continue;
        }
        if !in_workspace_dependencies || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() != dep_name {
            continue;
        }
        return Ok(Some(materialize_app_workspace_dependency(
            dep_name,
            value.trim(),
            &workspace_root,
        )));
    }

    Ok(None)
}

fn app_workspace_manifest(app_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    for ancestor in app_dir.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        if manifest_has_workspace_dependencies(&manifest) {
            return Some((ancestor.to_path_buf(), manifest));
        }
    }
    None
}

fn manifest_has_workspace_dependencies(manifest_path: &Path) -> bool {
    let Ok(cargo_toml) = fs::read_to_string(manifest_path) else {
        return false;
    };
    cargo_toml
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .any(|line| line == "[workspace.dependencies]")
}

fn materialize_app_workspace_dependency(
    dep_name: &str,
    value: &str,
    workspace_root: &Path,
) -> String {
    if let Some(path) = inline_table_path(value) {
        let path = resolve_manifest_path(workspace_root, &path);
        return path_dependency_line(dep_name, &path);
    }
    format!("{dep_name} = {value}")
}

fn path_dependency_line(dep_name: &str, path: &Path) -> String {
    format!(
        "{dep_name} = {{ path = {} }}",
        toml_string(&path.display().to_string())
    )
}

fn stage_trueos_tetris_crate(blueprint_root: &Path, work_dir: &Path) -> Result<PathBuf, String> {
    let source = blueprint_root.join("../uiout/crates/trueos-tetris");
    let staged = work_dir.join("blueprint-crates").join("trueos-tetris");
    reset_dir(&staged)?;
    copy_app_tree(&source, &staged)?;
    rewrite_manifest_dependency_path(
        &staged.join("Cargo.toml"),
        "trueos",
        &blueprint_root.join("api").display().to_string(),
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
        return Err(format!(
            "missing dependency `{dep_name}` in {}",
            manifest_path.display()
        ));
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
        &[
            "-fno-stack-protector",
            "-DROCKSDB_PLATFORM_POSIX",
            "-DROCKSDB_LIB_IO_POSIX",
            "-DOS_LINUX",
        ],
    );
    push_env_words(
        command,
        "CXXFLAGS",
        &[
            "-fno-stack-protector",
            "-DROCKSDB_PLATFORM_POSIX",
            "-DROCKSDB_LIB_IO_POSIX",
            "-DOS_LINUX",
        ],
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

fn toml_bool_value(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
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
