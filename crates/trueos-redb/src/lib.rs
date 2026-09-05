//! redb images backed by RAM, with file I/O left to the caller's async storage.
//! Finish transactions, consume the database into an image, then persist that
//! image. This backend does not promise disk durability before that final write.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, Ordering};
pub use redb;
#[cfg(not(feature = "std"))]
use redb::io::Error;
use redb::{Database, StorageBackend, backends::InMemoryBackend};
#[cfg(feature = "std")]
use std::io::Error;

#[derive(Clone, Debug)]
struct SharedBackend {
    memory: Arc<InMemoryBackend>,
    closed: Arc<AtomicBool>,
}

impl StorageBackend for SharedBackend {
    fn len(&self) -> Result<u64, Error> {
        self.memory.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), Error> {
        StorageBackend::read(&*self.memory, offset, out)
    }
    fn set_len(&self, len: u64) -> Result<(), Error> {
        self.memory.set_len(len)
    }
    fn sync_data(&self) -> Result<(), Error> {
        self.memory.sync_data()
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), Error> {
        StorageBackend::write(&*self.memory, offset, data)
    }
    fn close(&self) -> Result<(), Error> {
        self.memory.close()?;
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
}

#[derive(Debug)]
pub struct ImageDatabase {
    database: Database,
    backend: SharedBackend,
}

impl ImageDatabase {
    /// Empty bytes create a new database. Invalid nonempty images are errors.
    pub fn open(image: &[u8]) -> Result<Self, String> {
        let backend = SharedBackend {
            memory: Arc::new(InMemoryBackend::new()),
            closed: Arc::new(AtomicBool::new(false)),
        };
        backend
            .set_len(image.len() as u64)
            .map_err(|error| error.to_string())?;
        backend.write(0, image).map_err(|error| error.to_string())?;
        let database = Database::builder()
            .create_with_backend(backend.clone())
            .map_err(|error| error.to_string())?;
        Ok(Self { database, backend })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Close redb before exposing bytes, including its final header writes.
    /// A live write transaction can delay close; never publish that image.
    pub fn into_image(self) -> Result<Vec<u8>, String> {
        let Self { database, backend } = self;
        drop(database);
        if !backend.closed.load(Ordering::Acquire) {
            return Err("redb image still has an active transaction".into());
        }
        let len = usize::try_from(backend.len().map_err(|error| error.to_string())?)
            .map_err(|_| "redb image exceeds address space")?;
        let mut image = vec![0; len];
        backend
            .read(0, &mut image)
            .map_err(|error| error.to_string())?;
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::{ReadableDatabase, TableDefinition};
    const VALUES: TableDefinition<u64, u64> = TableDefinition::new("values");

    #[test]
    fn committed_values_survive_image_close_and_reopen() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        {
            write.open_table(VALUES).unwrap().insert(7, 13).unwrap();
        }
        write.commit().unwrap();
        let image = store.into_image().unwrap();
        let restored = ImageDatabase::open(&image).unwrap();
        assert_eq!(
            restored
                .database()
                .begin_read()
                .unwrap()
                .open_table(VALUES)
                .unwrap()
                .get(7)
                .unwrap()
                .unwrap()
                .value(),
            13
        );
    }

    #[test]
    fn aborted_transaction_does_not_enter_persisted_image() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        {
            write.open_table(VALUES).unwrap();
        }
        write.commit().unwrap();
        let write = store.database().begin_write().unwrap();
        {
            write.open_table(VALUES).unwrap().insert(7, 13).unwrap();
        }
        write.abort().unwrap();
        let restored = ImageDatabase::open(&store.into_image().unwrap()).unwrap();
        assert!(
            restored
                .database()
                .begin_read()
                .unwrap()
                .open_table(VALUES)
                .unwrap()
                .get(7)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn corrupt_image_is_not_replaced_with_an_empty_database() {
        assert!(ImageDatabase::open(b"invalid nonempty database image").is_err());
    }

    #[test]
    fn live_write_transaction_prevents_image_publication() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        assert!(store.into_image().is_err());
        write.abort().unwrap();
    }
}
