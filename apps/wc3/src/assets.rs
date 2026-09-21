//! Immutable session backing in Blueprint RAM. This module has no guest
//! AddressSpace or mapping capability; guest VA remains unchanged.
use std::{future::Future, sync::Arc};
use trueos::async_fs::{self, DirListing};

pub struct ResidentAsset {
    stored_path: String,
    bytes: Arc<Vec<u8>>,
}

impl ResidentAsset {
    pub fn stored_path(&self) -> &str {
        &self.stored_path
    }
    pub fn len(&self) -> u64 {
        file_len(self.bytes.len())
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

// Keep file lengths independent of the 32-bit guest address width.
fn file_len(byte_len: usize) -> u64 {
    byte_len as u64
}

#[derive(Default)]
pub struct Wc3AssetCache {
    war3_mpq: Option<ResidentAsset>,
}

impl Wc3AssetCache {
    pub fn war3_mpq(&self) -> Option<&ResidentAsset> {
        self.war3_mpq.as_ref()
    }

    pub fn lookup(&self, path: &str) -> Option<Arc<Vec<u8>>> {
        let basename = path.rsplit(['/', '\\']).next()?;
        if !basename.eq_ignore_ascii_case("war3.mpq") {
            return None;
        }
        self.war3_mpq.as_ref().map(|asset| Arc::clone(&asset.bytes))
    }

    /// Returns true only for the first successful load in this session.
    pub async fn preload_war3(&mut self, listing: &DirListing) -> Result<bool, String> {
        self.preload_with(listing, |path| async move {
            async_fs::read_file(path.as_bytes())
                .await
                .map_err(|error| format!("read {path}: TRUEOSFS {error}"))
        })
        .await
    }

    async fn preload_with<F, Fut>(&mut self, listing: &DirListing, read: F) -> Result<bool, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, String>>,
    {
        if self.war3_mpq.is_some() {
            return Ok(false);
        }
        if listing.truncated {
            return Err("Warcraft III directory listing truncated".into());
        }
        let stored = crate::child_loader::resolve_file(listing, "war3.mpq")?
            .ok_or("war3.mpq missing from Warcraft III directory")?;
        let stored_path = format!("/common/Warcraft III/{stored}");
        let bytes = read(stored_path.clone()).await?;
        // Move the Vec without copying its data; even the small Arc allocation
        // is fallible so allocation errors follow the normal failure path.
        let bytes = Arc::try_new(bytes).map_err(|_| "allocate resident MPQ Arc")?;
        self.war3_mpq = Some(ResidentAsset { stored_path, bytes });
        Ok(true)
    }
}

/// Future file backing only: no Win32 handle registration or guest mapping.
pub struct ResidentFileHandle {
    bytes: Arc<Vec<u8>>,
    pub cursor: u64,
}

impl ResidentFileHandle {
    pub fn new(bytes: Arc<Vec<u8>>) -> Self {
        Self { bytes, cursor: 0 }
    }
    pub fn len(&self) -> u64 {
        file_len(self.bytes.len())
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
crate::wc3_assets_tests_1!();
