use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::toolchain;

const BLUEPRINT_PAYLOAD_7Z: u16 = 2;
pub(crate) const BLUEPRINT_CAP_REPLICATABLE: u16 = 1 << 8;
pub(crate) const BLUEPRINT_CAP_ARGV_ENTRY_V1: u16 = 1 << 9;
const TRUEOS_ASSET_BUNDLE_MAGIC: &[u8; 4] = b"TRAS";
const TRUEOS_ASSET_BUNDLE_VERSION: u16 = 1;
const TRUEOS_ASSET_BUNDLE_FLAGS: u16 = 0;
const TRUEOS_ASSET_BUNDLE_FILE: &str = "assets.bundle";
const TRUEOS_ASSET_SECTION: &str = ".trueos.assets";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AssetBundleEntry {
    pub(crate) logical_path: String,
    pub(crate) bytes: Vec<u8>,
}

impl AssetBundleEntry {
    pub(crate) fn new(logical_path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            logical_path: logical_path.into(),
            bytes: bytes.into(),
        }
    }
}

const fn blueprint_header_flags(capability_flags: u16) -> u16 {
    BLUEPRINT_PAYLOAD_7Z | capability_flags
}

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
    let sysroot = toolchain::rust_sysroot().ok()?;
    let host = env::var("HOST").ok().or_else(rustc_host_triple)?;
    Some(sysroot.join("lib").join("rustlib").join(host).join("bin"))
}

fn rustc_host_triple() -> Option<String> {
    let output = toolchain::rustc_command().arg("-vV").output().ok()?;
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

#[derive(Debug)]
pub(crate) struct CargoRootArtifacts {
    pub(crate) objects: Vec<PathBuf>,
    pub(crate) rlink: PathBuf,
}

/// Resolve the exact root object set recorded by rustc's newest `.rlink`.
///
/// With one codegen unit rustc emits `<crate>-<hash>.o`; with multiple units it
/// emits many `<crate>-<hash>.*.rcgu.o` files. The `.rlink` is the authoritative
/// inventory for both forms and avoids accidentally collecting stale CGUs from
/// Cargo's persistent target directory.
pub(crate) fn latest_cargo_root_artifacts(
    dir: &Path,
    stem: &str,
) -> Result<CargoRootArtifacts, String> {
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
            .and_then(|rest| rest.strip_suffix(".rlink"))
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
    let rlink = best
        .map(|(_, path)| path)
        .ok_or_else(|| format!("missing root .rlink for {stem} in {}", dir.display()))?;
    let root_stem = rlink
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("bad root .rlink path: {}", rlink.display()))?;
    let single_object = format!("{root_stem}.o");
    let cgu_prefix = format!("{root_stem}.");
    let bytes = fs::read(&rlink).map_err(io_string)?;
    let mut objects = Vec::new();
    for token in printable_tokens(&bytes) {
        let path = PathBuf::from(token);
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let belongs_to_root =
            name == single_object || (name.starts_with(&cgu_prefix) && name.ends_with(".rcgu.o"));
        if belongs_to_root && path.is_file() && !objects.iter().any(|existing| existing == &path) {
            objects.push(path);
        }
    }
    objects.sort();
    if objects.is_empty() {
        return Err(format!(
            "root .rlink {} contains no build objects for {stem}",
            rlink.display()
        ));
    }
    Ok(CargoRootArtifacts { objects, rlink })
}

pub(crate) fn cargo_artifact_stem(name: &str) -> String {
    name.replace('-', "_")
}

pub(crate) fn collect_rlibs_for_rlink(
    rlink: &Path,
    deps_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !rlink.is_file() {
        return Err(format!("missing dependency metadata: {}", rlink.display()));
    }
    collect_rlibs_from_rlink(rlink, deps_dir)
}

fn collect_rlibs_from_rlink(rlink: &Path, deps_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let bytes = fs::read(rlink).map_err(io_string)?;
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
        Err(format!(
            "failed to read rlib dependencies from {}",
            rlink.display()
        ))
    } else {
        Ok(out)
    }
}

pub(crate) fn verify_abort_panic_runtime(rlibs: &[PathBuf]) -> Result<(), String> {
    let panic_abort = rlibs
        .iter()
        .filter(|path| is_rlib_for_crate(path, "panic_abort"))
        .collect::<Vec<_>>();
    let panic_unwind = rlibs
        .iter()
        .filter(|path| is_rlib_for_crate(path, "panic_unwind"))
        .collect::<Vec<_>>();

    if !panic_unwind.is_empty() {
        return Err(format!(
            "root .rlink selected panic_unwind for a panic=abort Blueprint: {}; \
             refusing to link mutually exclusive panic runtimes",
            panic_unwind
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if panic_abort.len() != 1 {
        return Err(format!(
            "root .rlink must select exactly one panic_abort archive for a panic=abort \
             Blueprint; found {}{}",
            panic_abort.len(),
            if panic_abort.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    panic_abort
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ));
    }
    Ok(())
}

fn is_rlib_for_crate(path: &Path, crate_name: &str) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let untagged = format!("lib{crate_name}.rlib");
    if file_name == untagged {
        return true;
    }
    file_name
        .strip_prefix(&format!("lib{crate_name}-"))
        .and_then(|suffix| suffix.strip_suffix(".rlib"))
        .is_some_and(|tag| !tag.is_empty())
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
                    if let Some((section, value, _)) = best_main_symbol_from_readelf(&stdout) {
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

pub(crate) fn entry_symbol_name(object: &Path) -> Option<String> {
    let mut readelf = tool_command(&["llvm-readelf", "readelf"]).ok()?;
    let output = readelf.arg("-Ws").arg(object).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    best_main_symbol_from_readelf(&stdout).map(|(_, _, name)| name)
}

fn best_main_symbol_from_readelf(stdout: &str) -> Option<(u32, u32, String)> {
    let mut rust_main: Option<(u32, u32, String)> = None;
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
        if name == "_start" {
            return Some((section, value, name.to_string()));
        }
        if name == "main" {
            return Some((section, value, name.to_string()));
        }
        let prefer_rust_main = match &rust_main {
            Some((_, _, best_name)) => name.len() < best_name.len(),
            None => true,
        };
        if looks_like_rust_main_symbol(name) && prefer_rust_main {
            rust_main = Some((section, value, name.to_string()));
        }
    }
    rust_main
}

/// Encodes the deterministic kernel-facing asset stream stored inside the
/// single-file 7z payload attached to a Blueprint ELF.
pub(crate) fn encode_trueos_asset_bundle(entries: &[AssetBundleEntry]) -> Result<Vec<u8>, String> {
    let count = u32::try_from(entries.len())
        .map_err(|_| "TRUEOS asset bundle contains more than u32::MAX entries".to_owned())?;
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        left.logical_path
            .as_bytes()
            .cmp(right.logical_path.as_bytes())
    });

    for entry in &sorted {
        validate_logical_asset_path(&entry.logical_path)?;
    }
    for duplicate in sorted.windows(2) {
        if duplicate[0].logical_path == duplicate[1].logical_path {
            return Err(format!(
                "duplicate TRUEOS asset path `{}`",
                duplicate[0].logical_path
            ));
        }
    }

    let encoded_lengths = sorted
        .iter()
        .map(|entry| {
            let path_len = u32::try_from(entry.logical_path.len()).map_err(|_| {
                format!(
                    "TRUEOS asset path exceeds u32::MAX bytes: {}",
                    entry.logical_path
                )
            })?;
            let data_len = u64::try_from(entry.bytes.len()).map_err(|_| {
                format!(
                    "TRUEOS asset data length does not fit u64: {}",
                    entry.logical_path
                )
            })?;
            Ok((path_len, data_len))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let bundle_size = checked_asset_bundle_size(u64::from(count), encoded_lengths.iter().copied())?;
    let mut bundle = Vec::with_capacity(bundle_size);
    bundle.extend_from_slice(TRUEOS_ASSET_BUNDLE_MAGIC);
    bundle.extend_from_slice(&TRUEOS_ASSET_BUNDLE_VERSION.to_le_bytes());
    bundle.extend_from_slice(&TRUEOS_ASSET_BUNDLE_FLAGS.to_le_bytes());
    bundle.extend_from_slice(&count.to_le_bytes());

    for (entry, (path_len, data_len)) in sorted.into_iter().zip(encoded_lengths) {
        bundle.extend_from_slice(&path_len.to_le_bytes());
        bundle.extend_from_slice(&data_len.to_le_bytes());
        bundle.extend_from_slice(&Sha256::digest(&entry.bytes));
        bundle.extend_from_slice(entry.logical_path.as_bytes());
        bundle.extend_from_slice(&entry.bytes);
    }

    debug_assert_eq!(bundle.len(), bundle_size);
    Ok(bundle)
}

/// Attaches a compressed TRAS asset bundle to `input_elf` and returns
/// `output_elf`.
///
/// The attached `.trueos.assets` section is deliberately non-SHF_ALLOC: the
/// loader consumes it as packaging metadata rather than mapping it into the
/// Blueprint's runtime image.
pub(crate) fn attach_trueos_asset_bundle(
    input_elf: &Path,
    output_elf: &Path,
    entries: &[AssetBundleEntry],
) -> Result<PathBuf, String> {
    if !input_elf.is_file() {
        return Err(format!("missing input ELF: {}", input_elf.display()));
    }
    if input_elf == output_elf {
        return Err(format!(
            "TRUEOS asset packaging requires distinct input and output ELF paths: {}",
            input_elf.display()
        ));
    }
    let output_parent = output_elf
        .parent()
        .ok_or_else(|| format!("missing parent dir for {}", output_elf.display()))?;
    if !output_parent.is_dir() {
        return Err(format!(
            "missing output directory for TRUEOS asset ELF: {}",
            output_parent.display()
        ));
    }

    let bundle = encode_trueos_asset_bundle(entries)?;
    let archive = asset_archive_path(output_elf)?;
    compress_trueos_asset_bundle_7z(&bundle, &archive)?;

    let mut objcopy = tool_command(&["llvm-objcopy", "objcopy"])?;
    objcopy
        .arg("--add-section")
        .arg(format!("{TRUEOS_ASSET_SECTION}={}", archive.display()))
        .arg("--set-section-flags")
        .arg(format!("{TRUEOS_ASSET_SECTION}=readonly,contents"))
        .arg(input_elf)
        .arg(output_elf);
    let attach_result = crate::run_command(&mut objcopy, "objcopy TRUEOS assets");
    if attach_result.is_err() {
        return attach_result.map(|()| output_elf.to_path_buf());
    }

    verify_trueos_asset_section(output_elf)?;
    checked_u32_container_size(
        fs::metadata(output_elf).map_err(io_string)?.len(),
        "packaged ELF",
    )?;
    fs::remove_file(&archive).map_err(|err| {
        format!(
            "failed to remove temporary TRUEOS asset archive {}: {err}",
            archive.display()
        )
    })?;
    Ok(output_elf.to_path_buf())
}

fn validate_logical_asset_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("TRUEOS asset path must not be empty".to_owned());
    }
    if path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err(format!(
            "TRUEOS asset path must be a normalized relative path: `{path}`"
        ));
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.bytes().any(|byte| byte < b' ' || byte == 0x7f)
        {
            return Err(format!(
                "TRUEOS asset path contains an unsafe component: `{path}`"
            ));
        }
    }
    Ok(())
}

fn checked_asset_bundle_size(
    entry_count: u64,
    entry_lengths: impl IntoIterator<Item = (u32, u64)>,
) -> Result<usize, String> {
    if entry_count > u64::from(u32::MAX) {
        return Err("TRUEOS asset bundle contains more than u32::MAX entries".to_owned());
    }

    // magic + version + flags + count
    let mut total = 4u64 + 2 + 2 + 4;
    for (path_len, data_len) in entry_lengths {
        // path_len + data_len + SHA-256 + path + data
        total = total
            .checked_add(4 + 8 + 32)
            .and_then(|value| value.checked_add(u64::from(path_len)))
            .and_then(|value| value.checked_add(data_len))
            .ok_or_else(|| "TRUEOS asset bundle size overflow".to_owned())?;
    }
    checked_u32_container_size(total, "uncompressed TRUEOS asset bundle")?;
    usize::try_from(total)
        .map_err(|_| "TRUEOS asset bundle size does not fit host usize".to_owned())
}

fn checked_u32_container_size(size: u64, label: &str) -> Result<u32, String> {
    u32::try_from(size).map_err(|_| {
        format!("{label} is {size} bytes, exceeding the current u32 Blueprint container limit")
    })
}

fn asset_archive_path(output_elf: &Path) -> Result<PathBuf, String> {
    let file_name = output_elf
        .file_name()
        .ok_or_else(|| format!("missing file name for {}", output_elf.display()))?;
    let mut archive_name = file_name.to_os_string();
    archive_name.push(".trueos-assets.7z");
    Ok(output_elf.with_file_name(archive_name))
}

fn compress_trueos_asset_bundle_7z(bundle: &[u8], archive: &Path) -> Result<(), String> {
    checked_u32_container_size(
        u64::try_from(bundle.len())
            .map_err(|_| "TRUEOS asset bundle length does not fit u64".to_owned())?,
        "uncompressed TRUEOS asset bundle",
    )?;
    if archive.exists() {
        fs::remove_file(archive).map_err(|err| {
            format!(
                "failed to replace TRUEOS asset archive {}: {err}",
                archive.display()
            )
        })?;
    }

    let mut seven_zip = tool_command(&["7z", "7zz"])?;
    seven_zip
        .arg("a")
        .arg("-t7z")
        .arg("-mx=9")
        .arg("-m0=LZMA2")
        .arg("-mmt=off")
        .arg("-ms=off")
        .arg("-mtc=off")
        .arg("-mtm=off")
        .arg("-mta=off")
        .arg("-bd")
        .arg("-y")
        .arg(archive)
        .arg(format!("-si{TRUEOS_ASSET_BUNDLE_FILE}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = seven_zip.spawn().map_err(|err| {
        format!(
            "7z TRUEOS asset compression failed to start for {}: {err}",
            archive.display()
        )
    })?;
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| "7z TRUEOS asset compression did not provide stdin".to_owned())
        .and_then(|mut stdin| {
            stdin
                .write_all(bundle)
                .map_err(|err| format!("failed to stream TRUEOS assets to 7z: {err}"))
        });
    let output = child
        .wait_with_output()
        .map_err(|err| format!("failed to wait for 7z TRUEOS asset compression: {err}"))?;
    write_result?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "7z TRUEOS asset compression failed with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    checked_u32_container_size(
        fs::metadata(archive).map_err(io_string)?.len(),
        "compressed TRUEOS asset archive",
    )?;
    Ok(())
}

fn verify_trueos_asset_section(elf: &Path) -> Result<(), String> {
    let mut readobj = tool_command(&["llvm-readobj"])?;
    let output =
        readobj.arg("--sections").arg(elf).output().map_err(|err| {
            format!("llvm-readobj TRUEOS asset verification failed to start: {err}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "llvm-readobj TRUEOS asset verification failed with status {}",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("llvm-readobj section output was not UTF-8: {err}"))?;
    let Some(has_alloc_flag) = trueos_asset_section_alloc_flag(&stdout) else {
        return Err(format!(
            "objcopy output {} is missing {TRUEOS_ASSET_SECTION}",
            elf.display()
        ));
    };
    if has_alloc_flag {
        return Err(format!(
            "objcopy marked {TRUEOS_ASSET_SECTION} SHF_ALLOC in {}; \
             asset packaging sections must not consume runtime image space",
            elf.display()
        ));
    }
    Ok(())
}

fn trueos_asset_section_alloc_flag(readobj: &str) -> Option<bool> {
    let mut in_section = false;
    let mut is_asset_section = false;
    let mut has_alloc_flag = false;
    for line in readobj.lines() {
        let trimmed = line.trim();
        if trimmed == "Section {" {
            in_section = true;
            is_asset_section = false;
            has_alloc_flag = false;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("Name: ") {
            is_asset_section = name
                .split_whitespace()
                .next()
                .is_some_and(|name| name == TRUEOS_ASSET_SECTION);
            continue;
        }
        if is_asset_section && trimmed.starts_with("SHF_ALLOC ") {
            has_alloc_flag = true;
            continue;
        }
        if trimmed == "}" {
            if is_asset_section {
                return Some(has_alloc_flag);
            }
            in_section = false;
        }
    }
    None
}

pub(crate) fn write_blueprint(
    out: &Path,
    stripped: &Path,
    entry_hint_hex: &str,
    capability_flags: u16,
) -> Result<(), String> {
    let raw = fs::read(stripped).map_err(io_string)?;
    let payload = compress_blueprint_payload(stripped)?;
    let entry = u64::from_str_radix(entry_hint_hex, 16).map_err(|err| err.to_string())?;
    let payload_len = checked_u32_container_size(
        u64::try_from(payload.len())
            .map_err(|_| "Blueprint payload length does not fit u64".to_owned())?,
        "Blueprint payload",
    )?;
    let raw_len = checked_u32_container_size(
        u64::try_from(raw.len())
            .map_err(|_| "Blueprint raw module length does not fit u64".to_owned())?,
        "Blueprint raw module",
    )?;

    let mut bytes = Vec::with_capacity(24 + payload.len());
    bytes.extend_from_slice(b"TRBP");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&blueprint_header_flags(capability_flags).to_le_bytes());
    bytes.extend_from_slice(&entry.to_le_bytes());
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.extend_from_slice(&raw_len.to_le_bytes());
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[test]
    fn root_rlink_selects_complete_multi_codegen_unit_object_set() {
        let temp = test_directory("multi-cgu-root");
        let deps = temp.join("deps");
        fs::create_dir_all(&deps).unwrap();
        let rlink = deps.join("pumpkin-1111111111111111.rlink");
        let first = deps.join("pumpkin-1111111111111111.alpha.codegen.rcgu.o");
        let second = deps.join("pumpkin-1111111111111111.beta.codegen.rcgu.o");
        let stale = deps.join("pumpkin-1111111111111111.stale.codegen.rcgu.o");
        for path in [&first, &second, &stale] {
            fs::write(path, []).unwrap();
        }
        fs::write(
            &rlink,
            format!("rlink\0{}\0{}\0", second.display(), first.display()),
        )
        .unwrap();

        let artifacts = latest_cargo_root_artifacts(&deps, "pumpkin").unwrap();

        assert_eq!(artifacts.objects, vec![first, second]);
        assert_eq!(artifacts.rlink, rlink);
        assert!(!artifacts.objects.contains(&stale));
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn root_rlink_controls_link_closure_and_preserves_generic_unwind() {
        let temp = test_directory("root-rlink");
        let deps = temp.join("deps");
        fs::create_dir_all(&deps).unwrap();
        let app_obj = deps.join("rustc_min-1111111111111111.o");
        let app_rlink = deps.join("rustc_min-1111111111111111.rlink");
        let dependency = deps.join("libdependency-2222222222222222.rlib");
        let unwind = deps.join("libunwind-3333333333333333.rlib");
        let panic_abort = deps.join("libpanic_abort-4444444444444444.rlib");
        let cached_panic_unwind = deps.join("libpanic_unwind-5555555555555555.rlib");
        for path in [
            &app_obj,
            &dependency,
            &unwind,
            &panic_abort,
            &cached_panic_unwind,
        ] {
            fs::write(path, []).unwrap();
        }
        fs::write(
            &app_rlink,
            format!(
                "rlink\0{}\0{}\0{}\0",
                dependency.display(),
                unwind.display(),
                panic_abort.display()
            ),
        )
        .unwrap();

        let rlibs = collect_rlibs_for_rlink(&app_rlink, &deps).unwrap();

        assert_eq!(rlibs, vec![dependency, unwind, panic_abort]);
        assert!(!rlibs.contains(&cached_panic_unwind));
        verify_abort_panic_runtime(&rlibs).unwrap();
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn abort_runtime_validation_rejects_unwind_runtime_and_requires_abort_runtime() {
        let panic_abort = PathBuf::from("/tmp/libpanic_abort-1111111111111111.rlib");
        let panic_unwind = PathBuf::from("/tmp/libpanic_unwind-2222222222222222.rlib");
        let unwind = PathBuf::from("/tmp/libunwind-3333333333333333.rlib");

        verify_abort_panic_runtime(&[unwind.clone(), panic_abort.clone()]).unwrap();

        let error =
            verify_abort_panic_runtime(&[panic_abort, panic_unwind, unwind.clone()]).unwrap_err();
        assert!(error.contains("selected panic_unwind"), "{error}");

        let error = verify_abort_panic_runtime(&[unwind]).unwrap_err();
        assert!(error.contains("exactly one panic_abort"), "{error}");
    }

    #[test]
    fn replicatable_capability_keeps_the_payload_encoding() {
        assert_eq!(blueprint_header_flags(BLUEPRINT_CAP_REPLICATABLE), 0x0102);
    }

    #[test]
    fn argv_entry_capability_keeps_the_payload_encoding() {
        assert_eq!(blueprint_header_flags(BLUEPRINT_CAP_ARGV_ENTRY_V1), 0x0202);
        assert_eq!(
            blueprint_header_flags(BLUEPRINT_CAP_REPLICATABLE | BLUEPRINT_CAP_ARGV_ENTRY_V1),
            0x0302
        );
    }

    #[test]
    fn asset_bundle_is_sorted_versioned_and_hashed() {
        let entries = [
            AssetBundleEntry::new("share/z.txt", b"z".to_vec()),
            AssetBundleEntry::new("bin/a", b"alpha".to_vec()),
        ];
        let encoded = encode_trueos_asset_bundle(&entries).unwrap();
        let reversed =
            encode_trueos_asset_bundle(&[entries[1].clone(), entries[0].clone()]).unwrap();
        assert_eq!(encoded, reversed);
        assert_eq!(&encoded[..4], b"TRAS");
        assert_eq!(u16_at(&encoded, 4), 1);
        assert_eq!(u16_at(&encoded, 6), 0);
        assert_eq!(u32_at(&encoded, 8), 2);

        let mut offset = 12;
        let path_len = usize::try_from(u32_at(&encoded, offset)).unwrap();
        offset += 4;
        let data_len = usize::try_from(u64_at(&encoded, offset)).unwrap();
        offset += 8;
        let expected_digest = Sha256::digest(b"alpha");
        assert_eq!(&encoded[offset..offset + 32], &expected_digest[..]);
        offset += 32;
        assert_eq!(&encoded[offset..offset + path_len], b"bin/a");
        offset += path_len;
        assert_eq!(&encoded[offset..offset + data_len], b"alpha");
    }

    #[test]
    fn asset_bundle_rejects_traversal_ambiguous_paths_and_duplicates() {
        for path in [
            "",
            ".",
            "..",
            "../escape",
            "a/../escape",
            "/absolute",
            "a//b",
            "a/",
            r"a\b",
            "C:/drive",
        ] {
            let error =
                encode_trueos_asset_bundle(&[AssetBundleEntry::new(path, Vec::new())]).unwrap_err();
            assert!(error.contains("asset path"), "{path:?}: {error}");
        }

        let error = encode_trueos_asset_bundle(&[
            AssetBundleEntry::new("bin/rustc", b"a".to_vec()),
            AssetBundleEntry::new("bin/rustc", b"b".to_vec()),
        ])
        .unwrap_err();
        assert!(error.contains("duplicate TRUEOS asset path"));
    }

    #[test]
    fn asset_bundle_rejects_u32_container_overflow() {
        let error = checked_asset_bundle_size(1, [(1, u64::from(u32::MAX))]).unwrap_err();
        assert!(error.contains("u32 Blueprint container limit"));
        let error = checked_asset_bundle_size(u64::from(u32::MAX) + 1, []).unwrap_err();
        assert!(error.contains("more than u32::MAX entries"));
    }

    #[test]
    fn asset_7z_is_deterministic_and_elf_section_is_not_allocated() {
        let temp = test_directory("asset-section");
        let bundle = encode_trueos_asset_bundle(&[
            AssetBundleEntry::new("bin/rustc", b"compiler".to_vec()),
            AssetBundleEntry::new("lib/sysroot.rmeta", b"metadata".to_vec()),
        ])
        .unwrap();
        let archive_a = temp.join("a.7z");
        let archive_b = temp.join("b.7z");
        compress_trueos_asset_bundle_7z(&bundle, &archive_a).unwrap();
        compress_trueos_asset_bundle_7z(&bundle, &archive_b).unwrap();
        assert_eq!(fs::read(&archive_a).unwrap(), fs::read(&archive_b).unwrap());
        assert_eq!(extract_asset_bundle(&archive_a), bundle);

        let input = temp.join("input.elf");
        fs::copy(env::current_exe().unwrap(), &input).unwrap();
        let output = temp.join("packaged.elf");
        let entries = [
            AssetBundleEntry::new("lib/sysroot.rmeta", b"metadata".to_vec()),
            AssetBundleEntry::new("bin/rustc", b"compiler".to_vec()),
        ];
        assert_eq!(
            attach_trueos_asset_bundle(&input, &output, &entries).unwrap(),
            output
        );
        verify_trueos_asset_section(&output).unwrap();

        let dumped_archive = temp.join("dumped.7z");
        let mut objcopy = tool_command(&["llvm-objcopy", "objcopy"]).unwrap();
        objcopy
            .arg("--dump-section")
            .arg(format!(
                "{TRUEOS_ASSET_SECTION}={}",
                dumped_archive.display()
            ))
            .arg(&output);
        crate::run_command(&mut objcopy, "objcopy dump TRUEOS assets").unwrap();
        assert_eq!(extract_asset_bundle(&dumped_archive), bundle);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn readobj_asset_flag_parser_distinguishes_allocated_sections() {
        let non_alloc = r#"
Section {
  Name: .trueos.assets (1)
  Flags [ (0x0)
  ]
}
"#;
        assert_eq!(trueos_asset_section_alloc_flag(non_alloc), Some(false));

        let allocated = r#"
Section {
  Name: .trueos.assets (1)
  Flags [ (0x2)
    SHF_ALLOC (0x2)
  ]
}
"#;
        assert_eq!(trueos_asset_section_alloc_flag(allocated), Some(true));
    }

    fn extract_asset_bundle(archive: &Path) -> Vec<u8> {
        let mut seven_zip = tool_command(&["7z", "7zz"]).unwrap();
        let output = seven_zip
            .arg("e")
            .arg("-so")
            .arg("-bd")
            .arg(archive)
            .arg(TRUEOS_ASSET_BUNDLE_FILE)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "7z extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn u64_at(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }

    fn test_directory(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "trueos-blueprint-artifact-test-{}-{label}-{unique}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        path
    }
}
