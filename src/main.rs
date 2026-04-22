use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

enum BuildTarget {
    Package,
    Example(String),
}

enum BuildFlavor {
    TokioStd,
    ThinNoStd,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("trueos-blueprint: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let mut build_target = BuildTarget::Package;
    let first_arg = args.next();
    let no_args = first_arg.is_none();
    let app_dir = match first_arg {
        Some(arg) if arg == "example" || arg == "--example" => {
            let Some(name) = args.next() else {
                return Err("missing example name after `example`".to_string());
            };
            build_target = BuildTarget::Example(
                name.into_string()
                    .map_err(|_| "example name must be valid UTF-8".to_string())?,
            );
            PathBuf::from(".")
        }
        Some(arg) => PathBuf::from(arg),
        None => PathBuf::from("."),
    };
    let app_dir = fs::canonicalize(&app_dir)
        .map_err(|err| format!("failed to resolve app dir {}: {err}", app_dir.display()))?;
    let manifest_path = app_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("missing Cargo.toml in {}", app_dir.display()));
    }
    if no_args {
        build_target = default_build_target(&manifest_path)?;
    }

    let build_flavor = build_flavor(&app_dir, &manifest_path, &build_target)?;

    let target_spec = default_target_spec(&app_dir)?;
    let target_name = target_spec
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("bad target spec path: {}", target_spec.display()))?
        .to_string();
    let tmp_dir = tempdir(&app_dir)?;
    let cargo_target_dir = tmp_dir.join("target");

    let mut cargo = Command::new("cargo");
    cargo
        .arg("+nightly")
        .arg("rustc")
        .arg("-Z")
        .arg(match build_flavor {
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
    if matches!(build_flavor, BuildFlavor::ThinNoStd) {
        cargo.arg("--no-default-features");
    }

    let output_name = match &build_target {
        BuildTarget::Package => package_name(&manifest_path)?,
        BuildTarget::Example(name) => {
            cargo.arg("--example").arg(name);
            name.clone()
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
        BuildTarget::Example(name) => latest_one(&target_dir.join("examples"), &format!("{name}-*.o"))?,
    };
    let rlibs = collect_rlibs(&deps_dir)?;

    let linked = tmp_dir.join("module.o");
    let stripped = tmp_dir.join("module.stripped.o");

    let mut ld = Command::new("ld");
    ld.arg("-r")
        .arg("--gc-sections")
        .arg("-e")
        .arg("main")
        .arg("-o")
        .arg(&linked)
        .arg(&app_obj);
    if !rlibs.is_empty() {
        ld.arg("--start-group");
        for rlib in &rlibs {
            ld.arg(rlib);
        }
        ld.arg("--end-group");
    }

    run_command(&mut ld, "ld")?;

    let entry_hint_hex = entry_hint_hex(&linked)?;

    run_command(
        Command::new("objcopy")
            .arg("--strip-debug")
            .arg("--strip-unneeded")
            .arg(&linked)
            .arg(&stripped),
        "objcopy",
    )?;

    let out = app_dir.join("dist").join(format!("{output_name}.bp"));
    fs::create_dir_all(out.parent().ok_or("bad output path")?).map_err(io_string)?;
    write_blueprint(&out, &stripped, &entry_hint_hex)?;
    println!("packed {} -> {}", app_obj.display(), out.display());
    Ok(())
}

fn run_command(cmd: &mut Command, label: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|err| format!("{label} failed to start: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {status}"))
    }
}

fn io_string(err: io::Error) -> String {
    err.to_string()
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

fn default_build_target(manifest_path: &Path) -> Result<BuildTarget, String> {
    if package_name(manifest_path)? != "trueos-blueprint" {
        return Ok(BuildTarget::Package);
    }

    let examples = example_names(manifest_path)?;
    match examples.as_slice() {
        [name] => Ok(BuildTarget::Example(name.clone())),
        _ if examples.iter().any(|name| name == "hello_world") => {
            Ok(BuildTarget::Example("hello_world".to_string()))
        }
        [] => Ok(BuildTarget::Package),
        _ => Err("multiple examples found; use `cargo bp --example <name>`".to_string()),
    }
}

fn example_names(manifest_path: &Path) -> Result<Vec<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut names = Vec::new();
    let mut in_example = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_example = trimmed == "[[example]]";
            continue;
        }
        if in_example && trimmed.starts_with("name") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            names.push(value.trim().trim_matches('"').to_string());
        }
    }
    Ok(names)
}

fn build_flavor(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: &BuildTarget,
) -> Result<BuildFlavor, String> {
    let source_path = match build_target {
        BuildTarget::Package => return Ok(BuildFlavor::TokioStd),
        BuildTarget::Example(name) => example_source_path(app_dir, manifest_path, name)?,
    };
    let source = fs::read_to_string(&source_path)
        .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
    if source.contains("trueos_blueprint") || source.contains("tokio::") {
        Ok(BuildFlavor::TokioStd)
    } else {
        Ok(BuildFlavor::ThinNoStd)
    }
}

fn example_source_path(app_dir: &Path, manifest_path: &Path, example_name: &str) -> Result<PathBuf, String> {
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
        } else if trimmed.starts_with("path") && let Some((_, value)) = trimmed.split_once('=') {
            current_path = Some(value.trim().trim_matches('"').to_string());
        }
    }

    if in_example
        && current_name.as_deref() == Some(example_name)
        && let Some(path) = current_path
    {
        return Ok(app_dir.join(path));
    }

    Err(format!("missing path for example {example_name} in {}", manifest_path.display()))
}

fn entry_hint_hex(linked: &Path) -> Result<String, String> {
    let output = Command::new("readelf")
        .arg("-Ws")
        .arg(linked)
        .output()
        .map_err(|err| format!("readelf failed to start: {err}"))?;
    if !output.status.success() {
        return Err(format!("readelf failed with status {}", output.status));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| "readelf output is not UTF-8")?;
    for line in stdout.lines() {
        let cols = line.split_whitespace().collect::<Vec<_>>();
        if cols.len() < 8 {
            continue;
        }
        if cols[3] == "FUNC" && cols[7] == "main" {
            let value = cols[1].trim_start_matches("0x");
            let section = cols[6].parse::<u32>().unwrap_or(0);
            let value = u32::from_str_radix(value, 16).unwrap_or(0);
            return Ok(format!("{section:08x}{value:08x}"));
        }
    }
    Ok(String::from("0000000000000000"))
}

fn write_blueprint(
    out: &Path,
    stripped: &Path,
    entry_hint_hex: &str,
) -> Result<(), String> {
    let raw = fs::read(stripped).map_err(io_string)?;
    let entry = u64::from_str_radix(entry_hint_hex, 16).map_err(|err| err.to_string())?;

    let mut bytes = Vec::with_capacity(24 + raw.len());
    bytes.extend_from_slice(b"TRBP");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&entry.to_le_bytes());
    bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&raw);
    fs::write(out, bytes).map_err(io_string)
}

fn tempdir(app_dir: &Path) -> Result<PathBuf, String> {
    let base = app_dir.join("target").join("trueos-blueprint-tmp");
    fs::create_dir_all(&base).map_err(io_string)?;
    for attempt in 0..1024u32 {
        let candidate = base.join(format!(
            "trueos-blueprint-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("failed to allocate temp dir".to_string())
}
