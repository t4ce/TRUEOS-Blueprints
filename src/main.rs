use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
struct ExampleSpec {
    name: String,
    required_features: Vec<String>,
}

enum BuildTarget {
    Package,
    Example(String),
}

enum BuildFlavor {
    TokioStd,
    ThinNoStd,
}

struct BuildSettings {
    flavor: BuildFlavor,
    has_global_allocator: bool,
    has_panic_handler: bool,
    needs_tokio_net: bool,
    extra_features: Vec<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("trueos-blueprint: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (app_dir, requested_apps) = parse_cli_args(&args)?;
    let app_dir = fs::canonicalize(&app_dir)
        .map_err(|err| format!("failed to resolve app dir {}: {err}", app_dir.display()))?;
    let manifest_path = app_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("missing Cargo.toml in {}", app_dir.display()));
    }

    if package_name(&manifest_path)? == "trueos-blueprint" {
        let requested_examples = if requested_apps.is_empty() {
            example_names(&manifest_path)?
        } else {
            requested_apps
        };

        if requested_examples.is_empty() {
            return build_one_target(&app_dir, &manifest_path, BuildTarget::Package, &[]);
        }

        for example_name in requested_examples {
            let required_features = example_required_features(&manifest_path, &example_name)?;
            build_one_target(
                &app_dir,
                &manifest_path,
                BuildTarget::Example(example_name),
                &required_features,
            )?;
        }
        return Ok(());
    }

    if !requested_apps.is_empty() {
        return Err("named apps are only supported from the trueos-blueprint root".to_string());
    }

    let build_target = BuildTarget::Package;
    let required_features = Vec::new();
    build_one_target(&app_dir, &manifest_path, build_target, &required_features)
}

fn parse_cli_args(args: &[std::ffi::OsString]) -> Result<(PathBuf, Vec<String>), String> {
    if args.is_empty() {
        return Ok((PathBuf::from("."), Vec::new()));
    }

    let first = PathBuf::from(&args[0]);
    if first.join("Cargo.toml").is_file() {
        if args.len() > 1 {
            return Err("directory mode does not accept app names".to_string());
        }
        return Ok((first, Vec::new()));
    }

    let mut app_names = Vec::with_capacity(args.len());
    for arg in args {
        app_names.push(
            arg.clone()
                .into_string()
                .map_err(|_| "app name must be valid UTF-8".to_string())?,
        );
    }

    Ok((PathBuf::from("."), app_names))
}

fn build_one_target(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: BuildTarget,
    required_features: &[String],
) -> Result<(), String> {
    let build_settings = build_settings(&app_dir, &manifest_path, &build_target)?;

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
    let cargo_target_dir = packer_target_dir
        .join("cargo")
        .join(sanitize_path_component(&output_name));
    fs::create_dir_all(&cargo_target_dir).map_err(io_string)?;

    let work_dir = workdir(&packer_target_dir, &output_name)?;
    reset_dir(&work_dir)?;

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
        .arg(&manifest_path);
    cargo.env("CARGO_TARGET_DIR", &cargo_target_dir);
    let mut extra_features = required_features.to_vec();
    for feature in &build_settings.extra_features {
        push_feature(&mut extra_features, feature);
    }
    if matches!(build_settings.flavor, BuildFlavor::ThinNoStd) {
        cargo.arg("--no-default-features");
        if !build_settings.has_global_allocator {
            push_feature(&mut extra_features, "thin-default-global-allocator");
        }
        if !build_settings.has_panic_handler {
            push_feature(&mut extra_features, "thin-default-panic-handler");
        }
    } else if build_settings.needs_tokio_net {
        push_feature(&mut extra_features, "tokio-net-probe");
    }
    if !extra_features.is_empty() {
        cargo.arg("--features").arg(extra_features.join(","));
    }

    match &build_target {
        BuildTarget::Package => {}
        BuildTarget::Example(name) => {
            cargo.arg("--example").arg(name);
        }
    };
    cargo.arg("--").arg("-Zno-link").arg("--emit=obj");

    run_command(&mut cargo, "cargo rustc")?;

    let target_dir = cargo_target_dir.join(&target_name).join("debug");
    let deps_dir = target_dir.join("deps");
    if !deps_dir.is_dir() {
        return Err(format!("missing deps dir: {}", deps_dir.display()));
    }

    let app_obj = match &build_target {
        BuildTarget::Package => latest_one(&deps_dir, &format!("{output_name}-*.o"))?,
        BuildTarget::Example(name) => {
            latest_one(&target_dir.join("examples"), &format!("{name}-*.o"))?
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

    let out = app_dir.join("dist").join(format!("{output_name}.bp"));
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

fn latest_one(dir: &Path, pattern: &str) -> Result<PathBuf, String> {
    let prefix = pattern
        .strip_suffix('*')
        .or_else(|| pattern.split_once('*').map(|(p, _)| p))
        .ok_or_else(|| format!("unsupported pattern: {pattern}"))?;
    let suffix = pattern.rsplit_once('*').map(|(_, s)| s).unwrap_or("");
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
        if !name.starts_with(prefix) || !name.ends_with(suffix) {
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
        .ok_or_else(|| format!("missing required build artifact in {}", dir.display()))
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
    for candidate in [app_dir.join("target.json"), app_dir.join("trueos-app.json")] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    for ancestor in app_dir.ancestors().skip(1) {
        let candidate = ancestor.join("target.json");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "cannot infer target spec from {}; expected target.json near the blueprint Cargo.toml",
        app_dir.display()
    ))
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
    Err(format!(
        "failed to read package name from {}",
        manifest_path.display()
    ))
}

fn example_names(manifest_path: &Path) -> Result<Vec<String>, String> {
    Ok(example_specs(manifest_path)?
        .into_iter()
        .map(|example| example.name)
        .collect())
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

fn build_settings(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: &BuildTarget,
) -> Result<BuildSettings, String> {
    let source_path = match build_target {
        BuildTarget::Package => package_source_path(app_dir)?,
        BuildTarget::Example(name) => example_source_path(app_dir, manifest_path, name)?,
    };
    let source = fs::read_to_string(&source_path)
        .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
    let needs_tokio_net = source_needs_tokio_net(&source);
    let flavor = if needs_tokio_net
        || source.contains("trueos_blueprint")
        || source.contains("trueos_blueprint::")
        || source.contains("tokio::")
    {
        BuildFlavor::TokioStd
    } else {
        BuildFlavor::ThinNoStd
    };
    let mut extra_features = blueprint_feature_directives(&source);
    if needs_tokio_net {
        push_feature(&mut extra_features, "tokio-net-probe");
    }
    Ok(BuildSettings {
        flavor,
        has_global_allocator: source.contains("#[global_allocator]"),
        has_panic_handler: source.contains("#[panic_handler]"),
        needs_tokio_net,
        extra_features,
    })
}

fn package_source_path(app_dir: &Path) -> Result<PathBuf, String> {
    for candidate in [app_dir.join("src/main.rs"), app_dir.join("src/lib.rs")] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "missing package source; expected src/main.rs or src/lib.rs under {}",
        app_dir.display()
    ))
}

fn source_needs_tokio_net(source: &str) -> bool {
    source.contains("tokio::net")
        || source.contains("trueos_blueprint::net")
        || source.contains("current_thread_net")
        || source.contains("net::TcpListener")
        || source.contains("net::TcpStream")
        || source.contains("net::UdpSocket")
        || source.contains("net::mio")
        || source.contains("mio::net")
        || source.contains("socket2::")
}

fn blueprint_feature_directives(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let Some((_, suffix)) = line.split_once("trueos-blueprint:") else {
            continue;
        };
        let Some((key, value)) = suffix.split_once('=') else {
            continue;
        };
        if key.trim() != "features" {
            continue;
        }
        for feature in parse_string_array(value.trim()) {
            push_feature(&mut out, &feature);
        }
    }
    out
}

fn example_source_path(
    app_dir: &Path,
    manifest_path: &Path,
    example_name: &str,
) -> Result<PathBuf, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_example = false;
    let mut current_name: Option<String> = None;
    let mut current_path: Option<String> = None;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_example
                && current_name.as_deref() == Some(example_name)
                && let Some(path) = current_path.take()
            {
                return Ok(app_dir.join(path));
            }
            in_example = trimmed == "[[example]]";
            if in_example {
                current_name = None;
                current_path = None;
            }
            continue;
        }
        if !in_example {
            continue;
        }
        if trimmed.starts_with("name") {
            if let Some((_, value)) = trimmed.split_once('=') {
                current_name = Some(value.trim().trim_matches('"').to_string());
            }
        } else if trimmed.starts_with("path")
            && let Some((_, value)) = trimmed.split_once('=')
        {
            current_path = Some(value.trim().trim_matches('"').to_string());
        }
    }

    if in_example
        && current_name.as_deref() == Some(example_name)
        && let Some(path) = current_path
    {
        return Ok(app_dir.join(path));
    }

    Err(format!(
        "missing path for example {example_name} in {}",
        manifest_path.display()
    ))
}

fn entry_hint_hex(linked: &Path) -> String {
    if let Ok(mut readelf) = tool_command(&["llvm-readelf", "readelf"]) {
        if let Ok(output) = readelf.arg("-Ws").arg(linked).output() {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    for line in stdout.lines() {
                        let cols = line.split_whitespace().collect::<Vec<_>>();
                        if cols.len() < 8 {
                            continue;
                        }
                        if cols[3] == "FUNC" && cols[7] == "main" {
                            let value = cols[1].trim_start_matches("0x");
                            let section = cols[6].parse::<u32>().unwrap_or(0);
                            let value = u32::from_str_radix(value, 16).unwrap_or(0);
                            return format!("{section:08x}{value:08x}");
                        }
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
                    for line in stdout.lines() {
                        let trimmed = line.trim();
                        if trimmed == "Symbol {" {
                            current_value = None;
                            current_section = None;
                            current_is_function = false;
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
                        if trimmed == "Name: main" && current_is_function {
                            return format!(
                                "{:08x}{:08x}",
                                current_section.unwrap_or(0),
                                current_value.unwrap_or(0)
                            );
                        }
                    }
                }
            }
        }
    }

    String::from("0000000000000000")
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
