use crate::imports::LauncherImport;

pub const IMAGE_BASE: u32 = 0x0040_0000;
pub const ENTRY_RVA: u32 = 0x2144;
pub const IMAGE_BYTES: usize = 0x44_000;
pub const HEADERS_BYTES: usize = 0x1_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeSection {
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportSymbol {
    Name(String),
    Ordinal(u16),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptor {
    pub module: String,
    pub symbol: ImportSymbol,
    pub iat_rva: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseRelocation {
    pub page_rva: u32,
    pub offset: u16,
    pub kind: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportTarget {
    Rva(u32),
    Forwarder(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportEntry {
    pub ordinal: u32,
    pub name: Option<String>,
    pub target: ExportTarget,
}

pub struct PeImage {
    pub image_base: u32,
    pub entry_rva: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub sections: Vec<PeSection>,
    pub imports: Vec<ImportDescriptor>,
    pub relocations: Vec<BaseRelocation>,
    pub exports: Vec<ExportEntry>,
    pub image: Vec<u8>,
}

pub struct Materialized {
    pub image: Vec<u8>,
    pub imports: Vec<LauncherImport>,
    pub image_base: u32,
    pub entry_rva: u32,
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(2).ok_or("PE offset overflow")?)
            .ok_or("PE truncated")?
            .try_into()
            .map_err(|_| "PE u16")?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or("PE offset overflow")?)
            .ok_or("PE truncated")?
            .try_into()
            .map_err(|_| "PE u32")?,
    ))
}

fn c_string(bytes: &[u8], offset: usize) -> Result<String, &'static str> {
    let tail = bytes.get(offset..).ok_or("PE string offset")?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("PE unterminated string")?;
    core::str::from_utf8(&tail[..end])
        .map(str::to_owned)
        .map_err(|_| "PE non-ASCII import")
}

/// Parse and materialize a PE32/i386 image without applying launcher policy.
pub fn parse(bytes: &[u8]) -> Result<PeImage, &'static str> {
    if bytes.get(..2) != Some(b"MZ") {
        return Err("PE DOS signature");
    }
    let pe = usize::try_from(u32_at(bytes, 0x3c)?).map_err(|_| "PE header offset")?;
    if bytes.get(pe..pe.checked_add(4).ok_or("PE header overflow")?) != Some(b"PE\0\0") {
        return Err("PE signature");
    }
    if u16_at(bytes, pe + 4)? != 0x14c {
        return Err("PE machine is not i386");
    }
    let sections = usize::from(u16_at(bytes, pe + 6)?);
    let optional_bytes = usize::from(u16_at(bytes, pe + 20)?);
    let optional = pe.checked_add(24).ok_or("PE optional overflow")?;
    if optional_bytes < 0xe0 || u16_at(bytes, optional)? != 0x10b {
        return Err("PE optional header is not PE32");
    }
    let entry_rva = u32_at(bytes, optional + 16)?;
    let image_base = u32_at(bytes, optional + 28)?;
    let size_of_image = u32_at(bytes, optional + 56)?;
    let size_of_headers = u32_at(bytes, optional + 60)?;
    let image_len = usize::try_from(size_of_image).map_err(|_| "PE image size")?;
    let headers_len = usize::try_from(size_of_headers).map_err(|_| "PE headers size")?;
    if image_len == 0 || headers_len > image_len || bytes.len() < headers_len {
        return Err("PE headers truncated");
    }
    let mut image = vec![0; image_len];
    image[..headers_len].copy_from_slice(&bytes[..headers_len]);
    let table = optional
        .checked_add(optional_bytes)
        .ok_or("PE section table overflow")?;
    for index in 0..sections {
        let section = table
            .checked_add(index.checked_mul(40).ok_or("PE section count")?)
            .ok_or("PE section offset overflow")?;
        let virtual_size = u32_at(bytes, section + 8)?;
        let virtual_address = u32_at(bytes, section + 12)?;
        let characteristics = u32_at(bytes, section + 36)?;
        let raw_size = u32_at(bytes, section + 16)?;
        let raw_offset = u32_at(bytes, section + 20)?;
        if virtual_address
            .checked_add(virtual_size.max(raw_size))
            .filter(|end| {
                usize::try_from(*end)
                    .ok()
                    .is_some_and(|end| end <= image_len)
            })
            .is_none()
            || raw_offset
                .checked_add(raw_size)
                .filter(|end| {
                    usize::try_from(*end)
                        .ok()
                        .is_some_and(|end| end <= bytes.len())
                })
                .is_none()
        {
            return Err("PE section range");
        }
        let va = usize::try_from(virtual_address).map_err(|_| "PE virtual address")?;
        let raw = usize::try_from(raw_size).map_err(|_| "PE raw size")?;
        let source = usize::try_from(raw_offset).map_err(|_| "PE raw offset")?;
        image[va..va + raw].copy_from_slice(&bytes[source..source + raw]);
    }
    let export_rva = u32_at(bytes, optional + 96)?;
    let export_size = u32_at(bytes, optional + 100)?;
    let mut exports = Vec::new();
    if export_rva != 0 && export_size != 0 {
        let export_end = export_rva.checked_add(export_size).ok_or("PE export range")?;
        if usize::try_from(export_end).ok().is_none_or(|end| end > image_len) {
            return Err("PE export directory range");
        }
        let directory = usize::try_from(export_rva).map_err(|_| "PE export RVA")?;
        let export_base = u32_at(&image, directory + 16)?;
        let functions = usize::try_from(u32_at(&image, directory + 20)?).map_err(|_| "PE export functions")?;
        let names = usize::try_from(u32_at(&image, directory + 24)?).map_err(|_| "PE export names")?;
        let function_table = usize::try_from(u32_at(&image, directory + 28)?).map_err(|_| "PE export function table")?;
        let name_table = usize::try_from(u32_at(&image, directory + 32)?).map_err(|_| "PE export name table")?;
        let name_ordinal_table = usize::try_from(u32_at(&image, directory + 36)?).map_err(|_| "PE export ordinal table")?;
        let mut export_names = vec![None; functions];
        for index in 0..names {
            let name_rva = usize::try_from(u32_at(&image, name_table + index * 4)?).map_err(|_| "PE export name")?;
            let function = usize::from(u16_at(&image, name_ordinal_table + index * 2)?);
            if function >= functions { return Err("PE export name ordinal"); }
            export_names[function] = Some(c_string(&image, name_rva)?);
        }
        for index in 0..functions {
            let target_rva = u32_at(&image, function_table + index * 4)?;
            if target_rva == 0 { continue; }
            let target = if target_rva >= export_rva && target_rva < export_end {
                ExportTarget::Forwarder(c_string(&image, usize::try_from(target_rva).map_err(|_| "PE forwarder")?)?)
            } else { ExportTarget::Rva(target_rva) };
            exports.push(ExportEntry { ordinal: export_base.checked_add(u32::try_from(index).map_err(|_| "PE export ordinal")?).ok_or("PE export ordinal")?, name: export_names[index].clone(), target });
        }
    }
    let import_rva =
        usize::try_from(u32_at(bytes, optional + 104)?).map_err(|_| "PE import RVA")?;
    let import_size =
        usize::try_from(u32_at(bytes, optional + 108)?).map_err(|_| "PE import size")?;
    let import_end = import_rva
        .checked_add(import_size)
        .filter(|end| {
            usize::try_from(*end)
                .ok()
                .is_some_and(|end| end <= image_len)
        })
        .ok_or("PE import directory range")?;
    if import_rva == 0 || import_size == 0 {
        return Err("PE import directory missing");
    }
    let mut imports = Vec::new();
    let mut descriptor = import_rva;
    while descriptor
        .checked_add(20)
        .filter(|end| *end <= import_end)
        .is_some()
    {
        let lookup =
            usize::try_from(u32_at(&image, descriptor)?).map_err(|_| "PE import lookup")?;
        let module_rva =
            usize::try_from(u32_at(&image, descriptor + 12)?).map_err(|_| "PE module RVA")?;
        let iat = usize::try_from(u32_at(&image, descriptor + 16)?).map_err(|_| "PE IAT")?;
        if lookup == 0 && module_rva == 0 && iat == 0 {
            break;
        }
        let module = c_string(&image, module_rva)?;
        let mut index = 0usize;
        loop {
            let slot = lookup
                .checked_add(index.checked_mul(4).ok_or("PE import index")?)
                .ok_or("PE lookup overflow")?;
            let lookup_value = u32_at(&image, slot)?;
            if lookup_value == 0 {
                break;
            }
            let symbol = if lookup_value & 0x8000_0000 != 0 {
                ImportSymbol::Ordinal((lookup_value & 0xffff) as u16)
            } else {
                let name_rva = usize::try_from(lookup_value).map_err(|_| "PE import name")?;
                ImportSymbol::Name(c_string(
                    &image,
                    name_rva.checked_add(2).ok_or("PE import name overflow")?,
                )?)
            };
            let iat_rva = u32::try_from(iat.checked_add(index * 4).ok_or("PE IAT overflow")?)
                .map_err(|_| "PE IAT RVA")?;
            if image
                .get(usize::try_from(iat_rva).unwrap_or(usize::MAX)..)
                .and_then(|tail| tail.get(..4))
                .is_none()
            {
                return Err("PE IAT outside image");
            }
            imports.push(ImportDescriptor {
                module: module.clone(),
                symbol,
                iat_rva,
            });
            index += 1;
        }
        descriptor += 20;
    }
    let mut relocations = Vec::new();
    let reloc_rva = u32_at(bytes, optional + 96 + 5 * 8)?;
    let reloc_size = u32_at(bytes, optional + 96 + 5 * 8 + 4)?;
    if reloc_rva != 0 && reloc_size >= 8 {
        let end = reloc_rva
            .checked_add(reloc_size)
            .ok_or("PE relocation range")?;
        if usize::try_from(end).ok().is_none_or(|end| end > image_len) {
            return Err("PE relocation directory range");
        }
        let mut cursor = reloc_rva;
        while cursor + 8 <= end {
            let page_rva = u32_at(&image, usize::try_from(cursor).unwrap())?;
            let block_size = u32_at(&image, usize::try_from(cursor + 4).unwrap())?;
            if block_size < 8 || cursor + block_size > end {
                return Err("PE relocation block");
            }
            let mut item = cursor + 8;
            while item + 2 <= cursor + block_size {
                let value = u16_at(&image, usize::try_from(item).unwrap())?;
                relocations.push(BaseRelocation {
                    page_rva,
                    offset: value & 0x0fff,
                    kind: value >> 12,
                });
                item += 2;
            }
            cursor += block_size;
        }
    }
    let sections = (0..sections)
        .map(|index| {
            let section = table + index * 40;
            Ok(PeSection {
                virtual_address: u32_at(bytes, section + 12)?,
                virtual_size: u32_at(bytes, section + 8)?,
                raw_offset: u32_at(bytes, section + 20)?,
                raw_size: u32_at(bytes, section + 16)?,
                characteristics: u32_at(bytes, section + 36)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PeImage {
        image_base,
        entry_rva,
        size_of_image,
        size_of_headers,
        sections,
        imports,
        relocations,
        exports,
        image,
    })
}

/// Launcher-facing materialization retains the historical import identity and
/// applies only the launcher's policy checks outside the generic parser.
pub fn materialize(bytes: &[u8]) -> Result<Materialized, &'static str> {
    let parsed = parse(bytes)?;
    if parsed.entry_rva != ENTRY_RVA
        || parsed.image_base != IMAGE_BASE
        || parsed.size_of_image as usize != IMAGE_BYTES
        || parsed.size_of_headers as usize != HEADERS_BYTES
        || !parsed.relocations.is_empty()
    {
        return Err("PE fixed launcher header mismatch");
    }
    let mut imports = Vec::new();
    for descriptor in parsed.imports {
        let symbol = match descriptor.symbol {
            ImportSymbol::Name(name) => name,
            ImportSymbol::Ordinal(_) => return Err("launcher ordinal import unsupported"),
        };
        imports.push(LauncherImport {
            id: u32::try_from(imports.len()).map_err(|_| "PE import count")?,
            module: descriptor.module,
            symbol,
            iat_rva: descriptor.iat_rva,
        });
    }
    if !imports
        .iter()
        .any(|item| item.module.eq_ignore_ascii_case("KERNEL32.dll") && item.symbol == "GetVersion")
    {
        return Err("PE expected GetVersion missing");
    }
    Ok(Materialized {
        image: parsed.image,
        imports,
        image_base: parsed.image_base,
        entry_rva: parsed.entry_rva,
    })
}
