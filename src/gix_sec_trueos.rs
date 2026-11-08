//! Git ownership trust follows the frozen native principal and owner-keyed
//! workspace. Public readability and matching bytes are never ownership proof.
use std::{io, path::Path};

pub fn is_path_owned_by_current_user(path: &Path) -> io::Result<bool> {
    // Git asks about an existing path. The private-path creation assertion also
    // accepts missing leaves, so check/canonicalize it before querying policy.
    let path = std::fs::canonicalize(path)?;
    match trueos::async_fs::block_on(trueos::async_fs::assert_private_path(
        path.as_os_str().as_encoded_bytes(),
    )) {
        Ok(()) => Ok(true),
        // This includes readable FFA paths, external trees and foreign owners.
        // Keep Git's ordinary reduced trust and explicit safe.directory policy.
        Err(-19) => Ok(false),
        Err(code) => Err(io::Error::new(
            match code {
                -1 | -4 | -6 => io::ErrorKind::InvalidInput,
                -8 => io::ErrorKind::NotFound,
                -20 => io::ErrorKind::Unsupported,
                _ => io::ErrorKind::Other,
            },
            format!("native Git ownership assertion failed ({code})"),
        )),
    }
}
