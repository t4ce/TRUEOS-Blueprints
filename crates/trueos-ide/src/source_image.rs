//! Source schema over the existing shared RAM-image redb backend.
use alloc::vec::Vec;
use crate::{Result, SourceDatabase};

pub struct SourceImage { pub database: SourceDatabase }
impl SourceImage {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let database = SourceDatabase::from_image(bytes)?;
        database.verify()?;
        Ok(Self { database })
    }
    /// Close redb before exposing the image, including its final header writes.
    pub fn into_bytes(self) -> Result<Vec<u8>> { self.database.into_image() }
}
