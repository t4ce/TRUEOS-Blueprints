// Embedded by the packer inside TRUEOS std::fs::read_dir. The async CABI
// returns TDL1 records; names are byte strings, not newline-delimited text.
fn trueos_directory_names(bytes: &[u8]) -> io::Result<Vec<crate::ffi::CString>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid TRUEOS directory listing");
    if bytes.len() < 12 || &bytes[..4] != b"TDL1" || bytes[4..8] != [0; 4] {
        return Err(invalid());
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if count > (bytes.len() - 12) / 4 {
        return Err(invalid());
    }
    let mut names = Vec::with_capacity(count);
    let mut offset = 12usize;
    for _ in 0..count {
        let end = offset.checked_add(4).ok_or_else(invalid)?;
        let header = bytes.get(offset..end).ok_or_else(invalid)?;
        if !matches!(header[0], 1 | 2) || header[1] != 0 {
            return Err(invalid());
        }
        let len = u16::from_le_bytes([header[2], header[3]]) as usize;
        offset = end;
        let end = offset.checked_add(len).ok_or_else(invalid)?;
        let name = bytes.get(offset..end).ok_or_else(invalid)?;
        if name.is_empty() || name.contains(&b'/') || matches!(name, b"." | b"..") {
            return Err(invalid());
        }
        names.push(crate::ffi::CString::new(name).map_err(|_| invalid())?);
        offset = end;
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    Ok(names)
}
