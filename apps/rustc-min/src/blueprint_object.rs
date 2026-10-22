//! Validation and publication of a compiler-produced TRUEOS Blueprint object.
//!
//! This module deliberately knows nothing about the source database. It accepts
//! one ELF relocatable object and can publish the corresponding TRBP v1 bytes
//! only to a filesystem path supplied by its caller.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use object::{
    Architecture, BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSymbol, RelocationFlags,
    RelocationTarget, SectionFlags, SymbolKind,
};

const BLUEPRINT_HEADER_LEN: usize = 24;
const BLUEPRINT_VERSION: u16 = 1;
const BLUEPRINT_PAYLOAD_RAW: u16 = 1;

/// Compiler/runtime imports which rustc may emit even when application-facing
/// imports use the `trueos_cabi_*` namespace.
const COMPILER_RUNTIME_IMPORTS: &[&str] = &[
    "memcpy",
    "memmove",
    "memset",
    "memcmp",
    "strlen",
    "__rust_alloc",
    "__rust_dealloc",
    "__rust_realloc",
    "__rust_alloc_zeroed",
    "__rust_alloc_error_handler",
    "__rust_no_alloc_shim_is_unstable_v2",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedObject {
    pub object_bytes: usize,
    pub entry_hint: u64,
    /// Unique, sorted undefined imports.
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedBlueprint {
    pub blueprint_bytes: usize,
    pub object_bytes: usize,
    pub entry_hint: u64,
    /// Unique, sorted undefined imports.
    pub imports: Vec<String>,
}

/// Validate a single x86_64 ELF REL object accepted by the TRUEOS Blueprint
/// loader and return the metadata needed for a TRBP v1 envelope.
pub fn validate_object(bytes: &[u8]) -> Result<ValidatedObject, String> {
    let file = object::File::parse(bytes)
        .map_err(|err| format!("parse compiler-produced object: {err}"))?;
    if file.format() != BinaryFormat::Elf
        || file.kind() != ObjectKind::Relocatable
        || file.architecture() != Architecture::X86_64
        || !file.is_64()
        || !file.is_little_endian()
    {
        return Err(format!(
            "compiler output is not ELF64 little-endian x86_64 REL: format={:?} kind={:?} arch={:?} is_64={} little_endian={}",
            file.format(),
            file.kind(),
            file.architecture(),
            file.is_64(),
            file.is_little_endian()
        ));
    }

    let imports = validate_imports(&file)?;
    validate_relocations(&file)?;
    let entry_hint = validate_entry(&file)?;

    Ok(ValidatedObject {
        object_bytes: bytes.len(),
        entry_hint,
        imports,
    })
}

/// Wrap a validated object in the raw-payload TRBP v1 envelope.
///
/// The returned bytes are not persisted by this function.
pub fn encode_blueprint(object_bytes: &[u8]) -> Result<(Vec<u8>, ValidatedObject), String> {
    let validated = validate_object(object_bytes)?;
    let payload_len = u32::try_from(object_bytes.len()).map_err(|_| {
        format!(
            "compiler-produced object is too large for Blueprint v1: {} bytes",
            object_bytes.len()
        )
    })?;

    let mut blueprint = Vec::with_capacity(BLUEPRINT_HEADER_LEN + object_bytes.len());
    blueprint.extend_from_slice(b"TRBP");
    blueprint.extend_from_slice(&BLUEPRINT_VERSION.to_le_bytes());
    blueprint.extend_from_slice(&BLUEPRINT_PAYLOAD_RAW.to_le_bytes());
    blueprint.extend_from_slice(&validated.entry_hint.to_le_bytes());
    blueprint.extend_from_slice(&payload_len.to_le_bytes());
    blueprint.extend_from_slice(&payload_len.to_le_bytes());
    blueprint.extend_from_slice(object_bytes);
    Ok((blueprint, validated))
}

/// Read one compiler-produced object and publish its TRBP v1 envelope to the
/// caller-provided filesystem path.
///
/// This is the only persistence operation in the module. It has no database
/// dependency and cannot store either the object or Blueprint in the source DB.
pub fn publish(object_path: &Path, output_path: &Path) -> Result<PublishedBlueprint, String> {
    if output_path.as_os_str().is_empty() {
        return Err(String::from(
            "caller-provided Blueprint output path is empty",
        ));
    }
    let object_bytes = fs::read(object_path).map_err(|err| {
        format!(
            "read compiler-produced object {}: {err}",
            object_path.display()
        )
    })?;
    let (blueprint, validated) = encode_blueprint(object_bytes.as_slice())?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create Blueprint output directory {}: {err}",
                parent.display()
            )
        })?;
    }
    fs::write(output_path, blueprint.as_slice()).map_err(|err| {
        format!(
            "publish compiler-produced Blueprint {}: {err}",
            output_path.display()
        )
    })?;
    let persisted = fs::read(output_path).map_err(|err| {
        format!(
            "verify compiler-produced Blueprint {}: {err}",
            output_path.display()
        )
    })?;
    if persisted != blueprint {
        return Err(format!(
            "compiler-produced Blueprint readback mismatch: {}",
            output_path.display()
        ));
    }

    Ok(PublishedBlueprint {
        blueprint_bytes: blueprint.len(),
        object_bytes: validated.object_bytes,
        entry_hint: validated.entry_hint,
        imports: validated.imports,
    })
}

fn validate_imports(file: &object::File<'_>) -> Result<Vec<String>, String> {
    let mut imports = BTreeSet::new();
    for symbol in file.symbols() {
        if !symbol.is_undefined() {
            continue;
        }
        let name = symbol
            .name()
            .map_err(|err| format!("compiler-produced object has a non-UTF-8 import: {err}"))?;
        // ELF symbol zero has no name and is not an import.
        if name.is_empty() {
            continue;
        }
        if !symbol.is_global() && !symbol.is_weak() {
            return Err(format!(
                "compiler-produced `{name}` import is neither global nor weak"
            ));
        }
        if !is_supported_import_name(name) {
            return Err(format!(
                "compiler-produced object has unsupported import `{name}`; \
                 application imports must use the trueos_cabi_* namespace"
            ));
        }
        imports.insert(name.to_owned());
    }
    Ok(imports.into_iter().collect())
}

fn is_supported_import_name(name: &str) -> bool {
    is_trueos_cabi_name(name)
        || COMPILER_RUNTIME_IMPORTS.contains(&name)
        || is_core_panic_bounds_check_import(name)
        || is_rustc_allocator_import(name)
}

fn is_trueos_cabi_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("trueos_cabi_") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_core_panic_bounds_check_import(name: &str) -> bool {
    name.starts_with("_R") && name.ends_with("_4core9panicking18panic_bounds_check")
}

/// Nightly Rust hashes the allocator shim name into a v0 mangled symbol. Match
/// only the stable rustc tail categories also recognized by the kernel loader.
fn is_rustc_allocator_import(name: &str) -> bool {
    let Some((_, rustc_tail)) = name.rsplit_once("___rustc") else {
        return false;
    };
    rustc_tail.contains("___rust_alloc")
        || rustc_tail.contains("___rust_dealloc")
        || rustc_tail.contains("___rust_realloc")
        || rustc_tail.contains("___rust_alloc_zeroed")
        || rustc_tail.contains("___rust_alloc_error_handler")
        || rustc_tail.contains("___rust_no_alloc_shim_is_unstable")
}

fn validate_entry(file: &object::File<'_>) -> Result<u64, String> {
    let mut entry = None;
    for symbol in file.symbols() {
        if symbol.name().ok() != Some("_start") || !symbol.is_definition() {
            continue;
        }
        if !symbol.is_global() || symbol.kind() != SymbolKind::Text {
            return Err(String::from(
                "compiler-produced `_start` is not a global text symbol",
            ));
        }
        let section_index = symbol
            .section_index()
            .ok_or_else(|| String::from("compiler-produced `_start` has no section"))?;
        let section_data = file
            .section_by_index(section_index)
            .map_err(|err| format!("read compiler-produced `_start` section: {err}"))?;
        let SectionFlags::Elf { sh_flags } = section_data.flags() else {
            return Err(String::from(
                "compiler-produced `_start` has non-ELF section flags",
            ));
        };
        let required_flags =
            u64::from(object::elf::SHF_ALLOC) | u64::from(object::elf::SHF_EXECINSTR);
        if sh_flags & required_flags != required_flags {
            return Err(String::from(
                "compiler-produced `_start` section is not allocatable executable code",
            ));
        }
        let end = symbol
            .address()
            .checked_add(symbol.size())
            .ok_or_else(|| String::from("compiler-produced `_start` extent overflows"))?;
        if symbol.address() >= section_data.size()
            || (symbol.size() != 0 && end > section_data.size())
        {
            return Err(String::from(
                "compiler-produced `_start` is outside its ELF section",
            ));
        }
        let section = u32::try_from(section_index.0)
            .map_err(|_| String::from("compiler-produced `_start` section exceeds u32"))?;
        let offset = u32::try_from(symbol.address())
            .map_err(|_| String::from("compiler-produced `_start` offset exceeds u32"))?;
        if section == 0 {
            return Err(String::from(
                "compiler-produced `_start` uses the undefined ELF section",
            ));
        }
        if entry.replace((section, offset)).is_some() {
            return Err(String::from(
                "compiler-produced object contains multiple `_start` definitions",
            ));
        }
    }

    let (section, offset) =
        entry.ok_or_else(|| String::from("compiler-produced object has no `_start` definition"))?;
    Ok((u64::from(section) << 32) | u64::from(offset))
}

fn validate_relocations(file: &object::File<'_>) -> Result<(), String> {
    for section in file.sections() {
        let SectionFlags::Elf { sh_flags } = section.flags() else {
            return Err(String::from(
                "compiler-produced ELF has non-ELF section flags",
            ));
        };
        if sh_flags & u64::from(object::elf::SHF_ALLOC) == 0 {
            continue;
        }
        for (offset, relocation) in section.relocations() {
            if relocation.has_implicit_addend() {
                return Err(format!(
                    "compiler-produced object uses unsupported REL relocation in section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                ));
            }
            if !matches!(
                relocation.target(),
                RelocationTarget::Symbol(_) | RelocationTarget::Absolute
            ) {
                return Err(format!(
                    "compiler-produced object uses unsupported relocation target in section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                ));
            }
            let RelocationFlags::Elf { r_type } = relocation.flags() else {
                return Err(format!(
                    "compiler-produced object uses a non-ELF relocation in section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                ));
            };
            if !matches!(r_type, 0 | 1 | 2 | 4 | 9 | 11 | 41 | 42) {
                return Err(format!(
                    "compiler-produced object uses loader-unsupported x86_64 relocation {r_type} in section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                ));
            }
            let write_width = match r_type {
                0 => 0,
                1 => 8,
                _ => 4,
            };
            let end = offset.checked_add(write_width).ok_or_else(|| {
                format!(
                    "compiler-produced relocation extent overflows in section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                )
            })?;
            if end > section.size() {
                return Err(format!(
                    "compiler-produced relocation writes outside section {:?} at 0x{offset:x}",
                    section.name().unwrap_or("<non-UTF-8>")
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BLUEPRINT_HEADER_LEN, is_core_panic_bounds_check_import, is_rustc_allocator_import,
        is_supported_import_name, is_trueos_cabi_name,
    };

    #[test]
    fn accepts_namespaced_cabi_imports() {
        assert!(is_trueos_cabi_name("trueos_cabi_write"));
        assert!(is_trueos_cabi_name("trueos_cabi_gridpaper_snapshot_submit"));
        assert!(!is_trueos_cabi_name("trueos_cabi_"));
        assert!(!is_trueos_cabi_name("trueos_cabi_bad-name"));
        assert!(!is_trueos_cabi_name("write"));
    }

    #[test]
    fn accepts_only_narrow_compiler_runtime_exceptions() {
        assert!(is_supported_import_name("memcpy"));
        assert!(is_supported_import_name("__rust_alloc"));
        assert!(!is_supported_import_name("socket"));
        assert!(!is_supported_import_name("arbitrary_dependency_symbol"));
    }

    #[test]
    fn recognizes_hashed_rustc_allocator_shims() {
        assert!(is_rustc_allocator_import(
            "_RNvCs75cmLyI1ip2_7___rustc12___rust_alloc"
        ));
        assert!(!is_rustc_allocator_import(
            "_RNvCs75cmLyI1ip2_7___rustc9something_else"
        ));
    }

    #[test]
    fn recognizes_bounds_check_runtime_import() {
        assert!(is_core_panic_bounds_check_import(
            "_RNvCs123_4core9panicking18panic_bounds_check"
        ));
        assert!(!is_core_panic_bounds_check_import(
            "_RNvCs123_4core9panicking5panic"
        ));
    }

    #[test]
    fn trbp_v1_header_layout_is_stable() {
        assert_eq!(BLUEPRINT_HEADER_LEN, 4 + 2 + 2 + 8 + 4 + 4);
    }
}
