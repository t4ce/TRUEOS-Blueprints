use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Err(err) = run() {
        eprintln!("trueos-pack: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let app_dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let app_dir = fs::canonicalize(&app_dir)
        .map_err(|err| format!("failed to resolve app dir {}: {err}", app_dir.display()))?;
    let manifest_path = app_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("missing Cargo.toml in {}", app_dir.display()));
    }

    run_command(
        Command::new("cargo")
            .arg("+nightly")
            .arg("rustc")
            .arg("-Z")
            .arg("build-std=core,compiler_builtins,alloc")
            .arg("-Z")
            .arg("json-target-spec")
            .arg("--target")
            .arg("trueos-app.json")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg("--")
            .arg("--emit=obj"),
        "cargo rustc",
    )?;

    let package_name = package_name(&manifest_path)?;
    let deps_dir = app_dir.join("target/trueos-app/debug/deps");
    if !deps_dir.is_dir() {
        return Err(format!("missing deps dir: {}", deps_dir.display()));
    }

    let app_obj = latest_one(&deps_dir, &format!("{package_name}-*.o"))?;
    let trueos_rlib = latest_one(&deps_dir, "libtrueos-*.rlib")?;
    let trueos_sys_rlib = latest_one(&deps_dir, "libtrueos_sys-*.rlib")?;
    let alloc_rlib = latest_one(&deps_dir, "liballoc-*.rlib")?;
    let core_rlib = latest_one(&deps_dir, "libcore-*.rlib")?;
    let compiler_builtins_rlib = latest_one(&deps_dir, "libcompiler_builtins-*.rlib")?;

    let tmp_dir = tempdir()?;
    let linked = tmp_dir.join("module.o");
    let stripped = tmp_dir.join("module.stripped.o");
    let payload_7z = tmp_dir.join("payload.7z");

    run_command(
        Command::new("ld")
            .arg("-r")
            .arg("--gc-sections")
            .arg("-e")
            .arg("main")
            .arg("-o")
            .arg(&linked)
            .arg(&app_obj)
            .arg(&trueos_rlib)
            .arg(&trueos_sys_rlib)
            .arg(&alloc_rlib)
            .arg(&core_rlib)
            .arg(&compiler_builtins_rlib),
        "ld",
    )?;

    let entry_hint_hex = entry_hint_hex(&linked)?;

    run_command(
        Command::new("objcopy")
            .arg("--strip-debug")
            .arg("--strip-unneeded")
            .arg(&linked)
            .arg(&stripped),
        "objcopy",
    )?;

    run_command(
        Command::new("7z")
            .arg("a")
            .arg("-t7z")
            .arg("-mx=9")
            .arg("-m0=LZMA2")
            .arg("-myx=0")
            .arg("-ms=off")
            .arg("-bd")
            .arg("-y")
            .arg("-siPAYLOAD.BIN")
            .arg(&payload_7z)
            .stdin(fs::File::open(&stripped).map_err(io_string)?),
        "7z",
    )?;

    let out = app_dir
        .join("target/trueos-app/debug")
        .join(format!("{package_name}.bp"));
    fs::create_dir_all(out.parent().ok_or("bad output path")?).map_err(io_string)?;
    write_blueprint(&out, &payload_7z, &stripped, &entry_hint_hex)?;
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
    payload_7z: &Path,
    stripped: &Path,
    entry_hint_hex: &str,
) -> Result<(), String> {
    let payload = fs::read(payload_7z).map_err(io_string)?;
    let raw = fs::read(stripped).map_err(io_string)?;
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

fn tempdir() -> Result<PathBuf, String> {
    let base = env::temp_dir();
    for attempt in 0..1024u32 {
        let candidate = base.join(format!("trueos-pack-{}-{attempt}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("failed to allocate temp dir".to_string())
}
