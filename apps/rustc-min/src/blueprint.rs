use std::fs;
use std::path::Path;

use object::{
    Architecture, BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSymbol, RelocationFlags,
    RelocationTarget, SectionFlags, SymbolKind,
};

const BLUEPRINT_HEADER_LEN: usize = 24;
const BLUEPRINT_VERSION: u16 = 1;
const BLUEPRINT_PAYLOAD_RAW: u16 = 1;
const GRIDPAPER_IMPORTS: &[&str] = &[
    "memcpy",
    "memset",
    "trueos_cabi_gridpaper_snapshot_submit",
    "trueos_cabi_poll_once",
    "trueos_cabi_sleep_ms",
    "trueos_cabi_write",
];

pub const OUTPUT_PATH: &str = "/common/localcompile/local.bp";

pub struct PublishedBlueprint {
    pub blueprint_bytes: usize,
    pub object_bytes: usize,
    pub entry_hint: u64,
}

pub fn publish(object_path: &Path) -> Result<PublishedBlueprint, String> {
    let object_bytes = fs::read(object_path).map_err(|err| {
        format!(
            "read compiler-produced object {}: {err}",
            object_path.display()
        )
    })?;
    let entry_hint = validate_object(object_bytes.as_slice())?;
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
    blueprint.extend_from_slice(&entry_hint.to_le_bytes());
    blueprint.extend_from_slice(&payload_len.to_le_bytes());
    blueprint.extend_from_slice(&payload_len.to_le_bytes());
    blueprint.extend_from_slice(object_bytes.as_slice());

    let output = Path::new(OUTPUT_PATH);
    let parent = output
        .parent()
        .ok_or_else(|| format!("Blueprint output has no parent: {}", output.display()))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("create local compile directory {}: {err}", parent.display()))?;
    fs::write(output, blueprint.as_slice()).map_err(|err| {
        format!(
            "publish compiler-produced Blueprint {}: {err}",
            output.display()
        )
    })?;
    let persisted = fs::read(output).map_err(|err| {
        format!(
            "verify compiler-produced Blueprint {}: {err}",
            output.display()
        )
    })?;
    if persisted != blueprint {
        return Err(format!(
            "compiler-produced Blueprint readback mismatch: {}",
            output.display()
        ));
    }

    Ok(PublishedBlueprint {
        blueprint_bytes: blueprint.len(),
        object_bytes: object_bytes.len(),
        entry_hint,
    })
}

fn validate_object(bytes: &[u8]) -> Result<u64, String> {
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

    validate_imports(&file)?;
    validate_relocations(&file)?;

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

fn validate_imports(file: &object::File<'_>) -> Result<(), String> {
    let mut imports = std::collections::BTreeSet::new();
    for symbol in file.symbols() {
        if !symbol.is_undefined() {
            continue;
        }
        let name = symbol
            .name()
            .map_err(|err| format!("compiler-produced object has a non-UTF-8 import: {err}"))?;
        if name.is_empty() {
            continue;
        }
        if !is_gridpaper_import(name) {
            return Err(format!(
                "compiler-produced object has unsupported import `{name}`"
            ));
        }
        if symbol.is_weak() || !symbol.is_global() {
            return Err(format!(
                "compiler-produced `{name}` import is not strong and global"
            ));
        }
        if !imports.insert(name) {
            return Err(format!(
                "compiler-produced object imports `{name}` more than once"
            ));
        }
    }
    if !imports.contains("trueos_cabi_gridpaper_snapshot_submit") {
        return Err(String::from(
            "compiler-produced object is missing the Gridpaper snapshot import",
        ));
    }
    Ok(())
}

fn is_gridpaper_import(name: &str) -> bool {
    GRIDPAPER_IMPORTS.contains(&name) || is_core_panic_bounds_check_import(name)
}

fn is_core_panic_bounds_check_import(name: &str) -> bool {
    name.starts_with("_R") && name.ends_with("_4core9panicking18panic_bounds_check")
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
