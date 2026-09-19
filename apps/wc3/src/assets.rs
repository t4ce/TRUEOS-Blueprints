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
mod tests {
    use super::*;
    use std::cell::Cell;
    use trueos::async_fs::{DirEntry, NodeKind};

    // Any accidental guest mapping during a cache operation crosses this ABI.
    static GUEST_MAP_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_x86_address_space_map_v1(
        _handle: u64,
        _guest_va: u32,
        _len: u32,
        _permissions: u32,
    ) -> i32 {
        GUEST_MAP_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        -1
    }

    #[test]
    fn synthetic_file_lengths_do_not_truncate_to_guest_width() {
        assert_eq!(file_len(600 * 1024 * 1024), 629145600u64);
        let large = u32::MAX as usize + 600 * 1024 * 1024;
        assert_eq!(file_len(large), u64::from(u32::MAX) + 629145600);
    }

    fn listing() -> DirListing {
        DirListing {
            entries: vec![DirEntry {
                name: "War3.MPQ".into(),
                kind: NodeKind::File,
            }],
            truncated: false,
        }
    }

    #[test]
    fn one_read_shared_allocation_independent_cursors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let reads = Cell::new(0);
            let data = vec![1, 2, 3];
            let original = data.as_ptr();
            let mut cache = Wc3AssetCache::default();
            assert!(
                cache
                    .preload_with(&listing(), |path| {
                        assert_eq!(path, "/common/Warcraft III/War3.MPQ");
                        reads.set(reads.get() + 1);
                        std::future::ready(Ok(data))
                    })
                    .await
                    .unwrap()
            );
            assert!(
                !cache
                    .preload_with(&listing(), |_| {
                        reads.set(reads.get() + 1);
                        std::future::ready(Err("must not read twice".into()))
                    })
                    .await
                    .unwrap()
            );
            let bytes = cache.lookup("war3.mpq").unwrap();
            assert_eq!(bytes.as_ptr(), original);
            for path in [
                "War3.mpq",
                "WAR3.MPQ",
                ".\\war3.mpq",
                "C:\\games\\Warcraft III\\war3.mpq",
                "/common/Warcraft III/war3.mpq",
            ] {
                assert!(Arc::ptr_eq(&bytes, &cache.lookup(path).unwrap()));
            }
            assert_eq!(reads.get(), 1);
            assert!(cache.lookup("war3.mpq.bak").is_none());
            assert!(cache.lookup("war3.mpq/").is_none());
            assert_eq!(
                cache.war3_mpq().unwrap().stored_path(),
                "/common/Warcraft III/War3.MPQ"
            );
            let mut a = ResidentFileHandle::new(Arc::clone(&bytes));
            let b = ResidentFileHandle::new(Arc::clone(&bytes));
            a.cursor = 600 * 1024 * 1024;
            assert_eq!(a.cursor, 629145600u64);
            a.cursor = u64::from(u32::MAX) + 600 * 1024 * 1024;
            assert!(a.cursor > u64::from(u32::MAX));
            assert_eq!(b.cursor, 0);
            assert!(Arc::ptr_eq(&a.bytes, &b.bytes));
            let _: u64 = a.len();
            assert_eq!(a.len(), 3);
            let mut fresh = Wc3AssetCache::default();
            assert!(
                fresh
                    .preload_with(&listing(), |_| {
                        reads.set(reads.get() + 1);
                        std::future::ready(Ok(vec![4]))
                    })
                    .await
                    .unwrap()
            );
            assert_eq!(reads.get(), 2);
            assert_eq!(GUEST_MAP_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn failed_read_leaves_no_resident_backing() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let mut cache = Wc3AssetCache::default();
                assert_eq!(
                    cache
                        .preload_with(&listing(), |_| std::future::ready(Err(
                            "out of memory".into()
                        )))
                        .await,
                    Err("out of memory".into())
                );
                assert!(cache.lookup("war3.mpq").is_none());
            });
    }
}
