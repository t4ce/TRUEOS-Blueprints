//! Session-only decoded tiles. REDB transactions never touch a filesystem.
use anyhow::{Result, ensure};
use image::RgbaImage;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition},
};

const TILES: TableDefinition<&str, &[u8]> = TableDefinition::new("decoded_tiles");
const TILE_BYTES: usize = 256 * 256 * 4;
const MAX_TILES: usize = 128; // 32 MiB of decoded pixels, plus REDB overhead.

pub struct RamCache {
    store: ImageDatabase,
    lru: VecDeque<(String, Instant)>,
    capacity: usize,
}

impl RamCache {
    pub fn new() -> Result<Self> {
        Self::with_capacity(MAX_TILES)
    }
    fn with_capacity(capacity: usize) -> Result<Self> {
        ensure!(capacity > 0, "RAM tile capacity must be nonzero");
        let store = ImageDatabase::open(&[]).map_err(|e| anyhow::anyhow!(e))?;
        let write = store.database().begin_write()?;
        {
            write.open_table(TILES)?;
        }
        write.commit()?;
        Ok(Self {
            store,
            lru: VecDeque::new(),
            capacity,
        })
    }
    pub fn get(&mut self, key: &str) -> Result<Option<RgbaImage>> {
        self.get_at(key, Instant::now())
    }
    fn get_at(&mut self, key: &str, now: Instant) -> Result<Option<RgbaImage>> {
        let Some(index) = self.lru.iter().position(|(k, _)| k == key) else {
            return Ok(None);
        };
        let (key, expires) = self.lru.remove(index).unwrap();
        if now >= expires {
            let write = self.store.database().begin_write()?;
            {
                write.open_table(TILES)?.remove(key.as_str())?;
            }
            write.commit()?;
            return Ok(None);
        }
        let read = self.store.database().begin_read()?;
        let table = read.open_table(TILES)?;
        let pixels = table.get(key.as_str())?.map(|value| value.value().to_vec());
        self.lru.push_back((key, expires));
        Ok(pixels.and_then(|pixels| RgbaImage::from_raw(256, 256, pixels)))
    }
    pub fn insert(&mut self, key: String, image: &RgbaImage, lifetime: Duration) -> Result<()> {
        ensure!(
            image.dimensions() == (256, 256) && image.as_raw().len() == TILE_BYTES,
            "invalid RAM tile dimensions"
        );
        if lifetime.is_zero() {
            return Ok(());
        }
        let expires = Instant::now() + lifetime;
        let victim = if self.lru.iter().any(|(k, _)| k == &key) || self.lru.len() < self.capacity {
            None
        } else {
            self.lru.front().map(|(k, _)| k.clone())
        };
        let write = self.store.database().begin_write()?;
        {
            let mut table = write.open_table(TILES)?;
            if let Some(victim) = &victim {
                table.remove(victim.as_str())?;
            }
            table.insert(key.as_str(), image.as_raw().as_slice())?;
        }
        write.commit()?;
        if let Some(victim) = victim {
            self.lru.retain(|(k, _)| k != &victim);
        }
        self.lru.retain(|(k, _)| k != &key);
        self.lru.push_back((key, expires));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn decoded_pixels_roundtrip_lru_eviction_and_session_isolation() {
        let mut cache = RamCache::with_capacity(2).unwrap();
        let tile = RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 255]));
        for key in ["a", "b"] {
            cache
                .insert(key.into(), &tile, Duration::from_secs(60))
                .unwrap();
        }
        assert_eq!(cache.get("a").unwrap().unwrap(), tile);
        cache
            .insert("c".into(), &tile, Duration::from_secs(60))
            .unwrap();
        assert!(cache.get("b").unwrap().is_none());
        assert!(cache.get("c").unwrap().is_some());
        assert!(RamCache::new().unwrap().get("a").unwrap().is_none());
    }
    #[test]
    fn expiration_and_replacement() {
        let mut cache = RamCache::with_capacity(1).unwrap();
        let tile = RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 255]));
        cache
            .insert("a".into(), &tile, Duration::from_secs(60))
            .unwrap();
        let replacement = RgbaImage::from_pixel(256, 256, Rgba([4, 5, 6, 255]));
        cache
            .insert("a".into(), &replacement, Duration::from_secs(60))
            .unwrap();
        assert_eq!(cache.get("a").unwrap().unwrap(), replacement);
        assert!(
            cache
                .get_at("a", Instant::now() + Duration::from_secs(61))
                .unwrap()
                .is_none()
        );
        assert_eq!(cache.lru.len(), 0);
    }
}
