//! Opt-in build settings and durable symbols for diagnostic stack captures.
//! The loader still receives the ordinary debug-stripped Blueprint payload.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::artifact::CargoRootArtifacts;
use crate::cargo_output::CargoNativeArchiveArtifact;
use crate::cli::CargoProfile;
use crate::{RUSTFLAGS_ENCODED_SEPARATOR, io_string};

const RUSTFLAGS: &[&str] = &[
    "-C",
    "force-frame-pointers=yes",
    "-C",
    "debuginfo=2",
    "-C",
    "split-debuginfo=off",
    "-C",
    "strip=none",
];

/// Called only for an explicit package metadata opt-in. Rust flags apply to
/// every guest unit, including build-std. Native flags cover compiled C/C++
/// dependencies such as ZSTD. Preserve the packer's flags and caller flags.
pub(crate) fn configure_cargo(cargo: &mut Command, profile: CargoProfile) {
    let mut encoded = command_env(cargo, "CARGO_ENCODED_RUSTFLAGS");
    for flag in RUSTFLAGS {
        if !encoded.is_empty() {
            encoded.push(RUSTFLAGS_ENCODED_SEPARATOR.to_string());
        }
        encoded.push(flag);
    }
    cargo.env("CARGO_ENCODED_RUSTFLAGS", encoded);
    for key in ["CFLAGS", "CXXFLAGS"] {
        let mut value = command_env(cargo, key);
        if !value.is_empty() {
            value.push(" ");
        }
        value.push("-g -fno-omit-frame-pointer");
        cargo.env(key, value);
    }
    let prefix = format!("CARGO_PROFILE_{}", profile.label().to_ascii_uppercase());
    cargo.env(format!("{prefix}_DEBUG"), "2");
    cargo.env(format!("{prefix}_STRIP"), "none");
    cargo.env(format!("{prefix}_SPLIT_DEBUGINFO"), "off");
}

fn command_env(command: &Command, key: &str) -> OsString {
    command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(key))
        .map(|(_, value)| value.unwrap_or_default().to_owned())
        .unwrap_or_else(|| env::var_os(key).unwrap_or_default())
}

/// Link order and source identity come from two separate authorities: rustc's
/// selected `.rlink` and Cargo's current produced/reused artifact messages.
pub(crate) fn retain_native_inventory(
    root: &CargoRootArtifacts,
    rlibs: &[PathBuf],
    artifacts: &[CargoNativeArchiveArtifact],
    work_dir: &Path,
) -> Result<(), String> {
    let mut identities = BTreeMap::new();
    for artifact in artifacts {
        if let Some(previous) = identities.insert(&artifact.path, artifact) {
            if previous != artifact {
                return Err(format!(
                    "conflicting diagnostic archive identity: {}",
                    artifact.path.display()
                ));
            }
        }
    }
    let selected = rlibs
        .iter()
        .map(|path| {
            identities.get(path).copied().ok_or_else(|| {
                format!(
                    "diagnostic link input absent from current Cargo stream: {}",
                    path.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let report = serde_json::json!({
        "schema_version": 1,
        "root_rlink": root.rlink,
        "root_objects": root.objects,
        "selected_archives_in_link_order": selected,
    });
    fs::write(
        work_dir.join("native-link-inputs.json"),
        serde_json::to_vec_pretty(&report).map_err(|err| err.to_string())?,
    )
    .map_err(io_string)
}

#[derive(Serialize)]
struct ArtifactRecord<'a> {
    path: &'a Path,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct SidecarManifest<'a> {
    schema_version: u32,
    cargo_profile: &'a str,
    requested_rustflags: &'a [&'a str],
    requested_native_flags: &'a str,
    blueprint: ArtifactRecord<'a>,
    debug_elf: ArtifactRecord<'a>,
    packed_elf: ArtifactRecord<'a>,
}

fn artifact_record(path: &Path) -> Result<ArtifactRecord<'_>, String> {
    let mut file = fs::File::open(path).map_err(io_string)?;
    let bytes = file.metadata().map_err(io_string)?.len();
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io_string)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(ArtifactRecord {
        path,
        bytes,
        sha256: format!("{:x}", digest.finalize()),
    })
}

/// Preserve the complete linked ELF and the exact ELF given to the container
/// writer. Their allocated sections have identical code/data, but stripping
/// can change section indices, so GDB mapping must use the packed ELF layout.
/// A hash manifest binds both sidecars to this particular Blueprint build.
pub(crate) fn retain_sidecars(
    blueprint: &Path,
    linked: &Path,
    packaged_elf: &Path,
    profile: CargoProfile,
) -> Result<(), String> {
    let debug_elf = blueprint.with_extension("debug.o");
    let packed_elf = blueprint.with_extension("packed.o");
    fs::copy(linked, &debug_elf).map_err(io_string)?;
    fs::copy(packaged_elf, &packed_elf).map_err(io_string)?;
    let manifest = SidecarManifest {
        schema_version: 1,
        cargo_profile: profile.label(),
        requested_rustflags: RUSTFLAGS,
        requested_native_flags: "-g -fno-omit-frame-pointer",
        blueprint: artifact_record(blueprint)?,
        debug_elf: artifact_record(&debug_elf)?,
        packed_elf: artifact_record(&packed_elf)?,
    };
    let manifest_path: PathBuf = blueprint.with_extension("debug.json");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(io_string)?;
    println!(
        "trueos-blueprint: diagnostic symbols: {}",
        debug_elf.display()
    );
    println!(
        "trueos-blueprint: diagnostic packed ELF: {}",
        packed_elf.display()
    );
    println!(
        "trueos-blueprint: diagnostic artifact binding: {}",
        manifest_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "blueprint-stack-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn diagnostic_flags_preserve_existing_rust_and_native_settings() {
        let mut command = Command::new("cargo");
        let original = [
            "--check-cfg=cfg(target_os,values(\"trueos\"))",
            "-C",
            "panic=abort",
            "-C",
            "opt-level=1",
        ]
        .join("\u{1f}");
        command
            .env("CARGO_ENCODED_RUSTFLAGS", &original)
            .env("CFLAGS", "-fno-stack-protector -DROCKSDB_PLATFORM_POSIX")
            .env("CXXFLAGS", "-fno-stack-protector -std=c++17");
        configure_cargo(&mut command, CargoProfile::Release);
        assert_eq!(
            command_env(&command, "CARGO_ENCODED_RUSTFLAGS"),
            OsString::from(format!("{original}\u{1f}{}", RUSTFLAGS.join("\u{1f}")))
        );
        assert_eq!(
            command_env(&command, "CFLAGS"),
            "-fno-stack-protector -DROCKSDB_PLATFORM_POSIX -g -fno-omit-frame-pointer"
        );
        assert_eq!(
            command_env(&command, "CXXFLAGS"),
            "-fno-stack-protector -std=c++17 -g -fno-omit-frame-pointer"
        );
        assert_eq!(command_env(&command, "CARGO_PROFILE_RELEASE_DEBUG"), "2");
        assert_eq!(command_env(&command, "CARGO_PROFILE_RELEASE_STRIP"), "none");
        assert_eq!(
            command_env(&command, "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO"),
            "off"
        );
    }

    // ELF64 section order/contents, rather than symbol addresses or section
    // indices, are the loader contract. This fixture has real DWARF and local
    // symbols and is linked with the production coalescing script.
    fn allocated_sections(path: &Path) -> Vec<(Vec<u8>, u32, u64, u64, u64, Vec<u8>)> {
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
        let u16_at = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let table = u64_at(40) as usize;
        let width = u16_at(58);
        let count = u16_at(60);
        let names = table + width * u16_at(62);
        let strings = u64_at(names + 24) as usize;
        let mut result = Vec::new();
        for index in 0..count {
            let section = table + width * index;
            let flags = u64_at(section + 8);
            if flags & 2 == 0 {
                continue;
            }
            let name_at = strings + u32_at(section) as usize;
            let name_len = bytes[name_at..].iter().position(|&byte| byte == 0).unwrap();
            let name = bytes[name_at..name_at + name_len].to_vec();
            let kind = u32_at(section + 4);
            let offset = u64_at(section + 24) as usize;
            let size = u64_at(section + 32);
            let alignment = u64_at(section + 48);
            let contents = if kind == 8 {
                Vec::new()
            } else {
                bytes[offset..offset + size as usize].to_vec()
            };
            result.push((name, kind, flags, alignment, size, contents));
        }
        result
    }

    #[test]
    fn debug_sidecars_keep_dwarf_locals_and_match_the_packed_allocated_image() {
        let root = temp_dir();
        let source = root.join("fixture.c");
        let object = root.join("fixture.o");
        let linked = root.join("module.o");
        let stripped = root.join("module.stripped.o");
        let blueprint = root.join("probe.bp");
        fs::write(&source, "static __attribute__((noinline)) int diagnostic_local(int x) { return x * 3; } int _start(void) { volatile int value = 4; return diagnostic_local(value); }").unwrap();
        crate::run_command(
            Command::new("cc")
                .args([
                    "-O1",
                    "-g",
                    "-fno-omit-frame-pointer",
                    "-fno-optimize-sibling-calls",
                    "-ffunction-sections",
                    "-c",
                ])
                .arg(&source)
                .arg("-o")
                .arg(&object),
            "compile debug fixture",
        )
        .unwrap();
        fs::write(root.join("coalesce.ld"), crate::REL_COALESCE_LINKER_SCRIPT).unwrap();
        let mut ld = crate::artifact::tool_command(&["ld.lld", "rust-lld", "ld"]).unwrap();
        crate::run_command(
            ld.args(["-r", "--gc-sections", "--undefined", "_start", "-T"])
                .arg(root.join("coalesce.ld"))
                .arg("-o")
                .arg(&linked)
                .arg(&object),
            "link debug fixture",
        )
        .unwrap();
        let mut objcopy =
            crate::artifact::tool_command(&["llvm-objcopy", "rust-objcopy", "objcopy"]).unwrap();
        crate::run_command(
            objcopy.arg("--strip-debug").arg(&linked).arg(&stripped),
            "strip debug fixture",
        )
        .unwrap();
        let mut readelf = crate::artifact::tool_command(&["llvm-readelf", "readelf"]).unwrap();
        let output = readelf.args(["-SW", "-sW"]).arg(&linked).output().unwrap();
        assert!(output.status.success());
        let details = String::from_utf8(output.stdout).unwrap();
        assert!(details.contains(".debug_info"));
        assert!(
            details
                .lines()
                .any(|line| line.contains("LOCAL") && line.contains("diagnostic_local"))
        );
        crate::artifact::write_blueprint(
            &blueprint,
            &stripped,
            &crate::artifact::entry_hint_hex(&linked),
            0,
        )
        .unwrap();
        retain_sidecars(&blueprint, &linked, &stripped, CargoProfile::Release).unwrap();
        assert_eq!(
            allocated_sections(&root.join("probe.debug.o")),
            allocated_sections(&root.join("probe.packed.o"))
        );
        assert_eq!(
            fs::read(root.join("probe.debug.o")).unwrap(),
            fs::read(&linked).unwrap()
        );
        assert_eq!(
            fs::read(root.join("probe.packed.o")).unwrap(),
            fs::read(&stripped).unwrap()
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("probe.debug.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["blueprint"]["sha256"],
            artifact_record(&blueprint).unwrap().sha256
        );
        assert_eq!(
            manifest["debug_elf"]["sha256"],
            artifact_record(&linked).unwrap().sha256
        );
        assert_eq!(
            manifest["packed_elf"]["sha256"],
            artifact_record(&stripped).unwrap().sha256
        );
        fs::remove_dir_all(root).unwrap();
    }
}
