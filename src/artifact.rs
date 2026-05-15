use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn tool_command(tool_names: &[&str]) -> Result<Command, String> {
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

pub(crate) fn latest_cargo_object(dir: &Path, stem: &str) -> Result<PathBuf, String> {
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

pub(crate) fn cargo_artifact_stem(name: &str) -> String {
    name.replace('-', "_")
}

pub(crate) fn collect_rlibs_for_object(
    app_obj: &Path,
    deps_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let Some(stem) = app_obj.file_stem().and_then(|value| value.to_str()) else {
        return Err(format!("bad app object path: {}", app_obj.display()));
    };
    let rlink = app_obj.with_file_name(format!("{stem}.rlink"));
    if !rlink.is_file() {
        return Err(format!(
            "missing dependency metadata for {}; expected {}",
            app_obj.display(),
            rlink.display()
        ));
    }

    let bytes = fs::read(&rlink).map_err(io_string)?;
    let mut out = Vec::new();
    for token in printable_tokens(&bytes) {
        let Some(path) = rlib_path_from_token(&token, deps_dir) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        if !out.iter().any(|existing| existing == &path) {
            out.push(path);
        }
    }

    if out.is_empty() {
        Err(format!("failed to read rlib dependencies from {}", rlink.display()))
    } else {
        Ok(out)
    }
}

fn rlib_path_from_token(token: &str, deps_dir: &Path) -> Option<PathBuf> {
    let suffix_idx = token.find(".rlib")?;
    let end = suffix_idx + ".rlib".len();
    let candidate = &token[..end];
    if let Some(path_start) = candidate.find('/') {
        return Some(PathBuf::from(&candidate[path_start..]));
    }

    let file_start = candidate.rfind("lib")?;
    Some(deps_dir.join(&candidate[file_start..]))
}

fn printable_tokens(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = None;
    for (idx, byte) in bytes.iter().copied().enumerate() {
        if byte.is_ascii_graphic() {
            start.get_or_insert(idx);
        } else if let Some(token_start) = start.take()
            && idx > token_start
            && let Ok(token) = std::str::from_utf8(&bytes[token_start..idx])
        {
            out.push(token.to_string());
        }
    }
    if let Some(token_start) = start
        && let Ok(token) = std::str::from_utf8(&bytes[token_start..])
    {
        out.push(token.to_string());
    }
    out
}

pub(crate) fn entry_hint_hex(linked: &Path) -> String {
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

pub(crate) fn write_blueprint(
    out: &Path,
    stripped: &Path,
    entry_hint_hex: &str,
) -> Result<(), String> {
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
    crate::run_command(&mut seven_zip, "7z")?;
    fs::read(&archive).map_err(io_string)
}

fn io_string(err: io::Error) -> String {
    err.to_string()
}
