use crate::imports::LauncherImport;

pub const IMAGE_BASE: u32 = 0x0040_0000;
pub const ENTRY_RVA: u32 = 0x2144;
pub const IMAGE_BYTES: usize = 0x44_000;
pub const HEADERS_BYTES: usize = 0x1_000;

pub struct Materialized {
    pub image: Vec<u8>,
    pub imports: Vec<LauncherImport>,
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

/// Materialize the fixed Warcraft III 1.00 launcher image and its import table.
pub fn materialize(bytes: &[u8]) -> Result<Materialized, &'static str> {
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
    if u32_at(bytes, optional + 16)? != ENTRY_RVA
        || u32_at(bytes, optional + 28)? != IMAGE_BASE
        || usize::try_from(u32_at(bytes, optional + 56)?).ok() != Some(IMAGE_BYTES)
        || usize::try_from(u32_at(bytes, optional + 60)?).ok() != Some(HEADERS_BYTES)
    {
        return Err("PE fixed launcher header mismatch");
    }
    for directory in [5usize, 9] {
        let offset = optional + 96 + directory * 8;
        if u32_at(bytes, offset)? != 0 || u32_at(bytes, offset + 4)? != 0 {
            return Err("PE unsupported relocation or TLS directory");
        }
    }
    if bytes.len() < HEADERS_BYTES {
        return Err("PE headers truncated");
    }
    let mut image = vec![0; IMAGE_BYTES];
    image[..HEADERS_BYTES].copy_from_slice(&bytes[..HEADERS_BYTES]);
    let table = optional
        .checked_add(optional_bytes)
        .ok_or("PE section table overflow")?;
    for index in 0..sections {
        let section = table
            .checked_add(index.checked_mul(40).ok_or("PE section count")?)
            .ok_or("PE section offset overflow")?;
        let virtual_size =
            usize::try_from(u32_at(bytes, section + 8)?).map_err(|_| "PE virtual size")?;
        let virtual_address =
            usize::try_from(u32_at(bytes, section + 12)?).map_err(|_| "PE virtual address")?;
        let raw_size = usize::try_from(u32_at(bytes, section + 16)?).map_err(|_| "PE raw size")?;
        let raw_offset =
            usize::try_from(u32_at(bytes, section + 20)?).map_err(|_| "PE raw offset")?;
        if virtual_address
            .checked_add(virtual_size.max(raw_size))
            .filter(|end| *end <= IMAGE_BYTES)
            .is_none()
            || raw_offset
                .checked_add(raw_size)
                .filter(|end| *end <= bytes.len())
                .is_none()
        {
            return Err("PE section range");
        }
        image[virtual_address..virtual_address + raw_size]
            .copy_from_slice(&bytes[raw_offset..raw_offset + raw_size]);
    }
    let import_rva =
        usize::try_from(u32_at(bytes, optional + 104)?).map_err(|_| "PE import RVA")?;
    let import_size =
        usize::try_from(u32_at(bytes, optional + 108)?).map_err(|_| "PE import size")?;
    let import_end = import_rva
        .checked_add(import_size)
        .filter(|end| *end <= IMAGE_BYTES)
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
            let name_rva = usize::try_from(u32_at(&image, slot)?).map_err(|_| "PE import name")?;
            if name_rva == 0 {
                break;
            }
            if name_rva & 0x8000_0000 != 0 {
                return Err("PE ordinal import unsupported");
            }
            let symbol = c_string(
                &image,
                name_rva.checked_add(2).ok_or("PE import name overflow")?,
            )?;
            let iat_rva = u32::try_from(iat.checked_add(index * 4).ok_or("PE IAT overflow")?)
                .map_err(|_| "PE IAT RVA")?;
            if image
                .get(usize::try_from(iat_rva).unwrap_or(usize::MAX)..)
                .and_then(|tail| tail.get(..4))
                .is_none()
            {
                return Err("PE IAT outside image");
            }
            imports.push(LauncherImport {
                id: u32::try_from(imports.len()).map_err(|_| "PE import count")?,
                module: module.clone(),
                symbol,
                iat_rva,
            });
            index += 1;
        }
        descriptor += 20;
    }
    if !imports
        .iter()
        .any(|item| item.module.eq_ignore_ascii_case("KERNEL32.dll") && item.symbol == "GetVersion")
    {
        return Err("PE expected GetVersion missing");
    }
    Ok(Materialized { image, imports })
}
