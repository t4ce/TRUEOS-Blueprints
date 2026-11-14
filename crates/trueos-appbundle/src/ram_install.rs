use super::{MAX_MEMBERS, valid_archive};
use alloc::{string::String, vec::Vec};

pub const RAM_INSTALL_VFILE: &[u8] = b"vFile:appdb-install-ram";
pub const MAX_INSTALL_BYTES: usize = 512 * 1024 * 1024;

/// RAM install wire format: ADR1 followed by name length (u16 LE), body
/// length (u32 LE), UTF-8 archive name and Blueprint bytes, repeated to EOF.
pub fn append_ram_install(out: &mut Vec<u8>, name: &str, bytes: &[u8]) -> Result<(), String> {
    if !valid_archive(name) || bytes.is_empty() {
        return Err("Invalid RAM Blueprint".into());
    }
    let header = if out.is_empty() { 4 } else { 0 };
    if bytes.len() > u32::MAX as usize
        || out
            .len()
            .saturating_add(header + 6 + name.len())
            .saturating_add(bytes.len())
            > MAX_INSTALL_BYTES
    {
        return Err("RAM AppDB install too large".into());
    }
    if out.is_empty() {
        out.extend_from_slice(b"ADR1");
    }
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

pub fn decode_ram_install(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    if bytes.len() > MAX_INSTALL_BYTES || !bytes.starts_with(b"ADR1") {
        return Err("Invalid RAM AppDB install".into());
    }
    let mut remaining = &bytes[4..];
    let mut downloads: Vec<(String, Vec<u8>)> = Vec::new();
    while !remaining.is_empty() {
        if remaining.len() < 6 || downloads.len() == MAX_MEMBERS {
            return Err("Invalid RAM install batch".into());
        }
        let name_len = u16::from_le_bytes(remaining[..2].try_into().unwrap()) as usize;
        let body_len = u32::from_le_bytes(remaining[2..6].try_into().unwrap()) as usize;
        remaining = &remaining[6..];
        if name_len > remaining.len() || body_len == 0 || body_len > remaining.len() - name_len {
            return Err("Truncated RAM Blueprint".into());
        }
        let name =
            core::str::from_utf8(&remaining[..name_len]).map_err(|_| "Invalid archive UTF-8")?;
        if !valid_archive(name)
            || downloads
                .iter()
                .any(|(other, _)| other.eq_ignore_ascii_case(name))
        {
            return Err("Invalid or duplicate RAM Blueprint".into());
        }
        downloads.push((
            name.into(),
            remaining[name_len..name_len + body_len].to_vec(),
        ));
        remaining = &remaining[name_len + body_len..];
    }
    if downloads.is_empty() {
        return Err("Empty RAM install batch".into());
    }
    Ok(downloads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_bundle_round_trips_binary_blueprints() {
        let mut payload = Vec::new();
        append_ram_install(&mut payload, "helper.bp", &[0, 255, 10]).unwrap();
        append_ram_install(&mut payload, "app.bp", b"TRBP").unwrap();
        assert_eq!(
            decode_ram_install(&payload).unwrap(),
            [
                ("helper.bp".into(), alloc::vec![0, 255, 10]),
                ("app.bp".into(), b"TRBP".to_vec())
            ]
        );
        for length in 0..payload.len() {
            // A complete first member is a valid independent batch.
            if length != 4 + 6 + "helper.bp".len() + 3 {
                assert!(decode_ram_install(&payload[..length]).is_err(), "{length}");
            }
        }
    }

    #[test]
    fn ram_install_rejects_duplicates_invalid_names_empty_and_excess_members() {
        let mut payload = Vec::new();
        assert!(append_ram_install(&mut payload, "../app.bp", b"x").is_err());
        assert!(append_ram_install(&mut payload, "app.bp", b"").is_err());
        assert!(decode_ram_install(b"ADR1").is_err());
        append_ram_install(&mut payload, "app.bp", b"x").unwrap();
        append_ram_install(&mut payload, "APP.bp", b"x").unwrap();
        assert!(decode_ram_install(&payload).is_err());
        payload.clear();
        for i in 0..=MAX_MEMBERS {
            append_ram_install(&mut payload, &alloc::format!("app{i}.bp"), b"x").unwrap();
        }
        assert!(decode_ram_install(&payload).is_err());
    }
}
