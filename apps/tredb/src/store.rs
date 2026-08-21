use std::fmt;

use redb::{
    backends::InMemoryBackend, Database, ReadableDatabase, ReadableTable, TableDefinition,
    TableHandle,
};

use crate::model::{DbSnapshot, RowSnapshot, TableSnapshot};

const MAX_ROWS_PER_TABLE: usize = 512;

type RawTable<'a> = TableDefinition<'a, &'static [u8], &'static [u8]>;

#[derive(Debug)]
pub struct Store {
    db: Database,
}

impl Store {
    pub fn new(seed_demo: bool) -> Result<Self, StoreError> {
        let mut store = Self {
            db: create_database()?,
        };
        if seed_demo {
            store.seed_demo()?;
        }
        Ok(store)
    }

    pub fn reset(&mut self, seed_demo: bool) -> Result<(), StoreError> {
        let mut replacement = Self {
            db: create_database()?,
        };
        if seed_demo {
            replacement.seed_demo()?;
        }
        self.db = replacement.db;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<DbSnapshot, StoreError> {
        let transaction = self.db.begin_read().map_err(StoreError::redb)?;
        let handles = transaction.list_tables().map_err(StoreError::redb)?;
        let mut names: Vec<String> = handles.map(|handle| handle.name().to_owned()).collect();
        names.sort();

        let mut tables = Vec::with_capacity(names.len());
        for name in names {
            let definition = raw_table(&name)?;
            let table = transaction
                .open_table(definition)
                .map_err(StoreError::redb)?;
            let mut rows = Vec::new();
            let mut truncated = false;
            for item in table.iter().map_err(StoreError::redb)? {
                if rows.len() == MAX_ROWS_PER_TABLE {
                    truncated = true;
                    break;
                }
                let (key, value) = item.map_err(StoreError::redb)?;
                rows.push(RowSnapshot {
                    key: key.value().to_vec(),
                    value: value.value().to_vec(),
                });
            }
            tables.push(TableSnapshot {
                name,
                rows,
                truncated,
            });
        }

        Ok(DbSnapshot { tables })
    }

    pub fn create_table(&self, requested_name: &str) -> Result<String, StoreError> {
        let name = normalize_table_name(requested_name)?;
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        {
            let definition = raw_table(&name)?;
            let _table = transaction
                .open_table(definition)
                .map_err(StoreError::redb)?;
        }
        transaction.commit().map_err(StoreError::redb)?;
        Ok(name)
    }

    pub fn delete_table(&self, table_name: &str) -> Result<bool, StoreError> {
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        let deleted = transaction
            .delete_table(raw_table(table_name)?)
            .map_err(StoreError::redb)?;
        transaction.commit().map_err(StoreError::redb)?;
        Ok(deleted)
    }

    pub fn upsert(&self, table_name: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_key(key)?;
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        {
            let mut table = transaction
                .open_table(raw_table(table_name)?)
                .map_err(StoreError::redb)?;
            let old = table.insert(key, value).map_err(StoreError::redb)?;
            drop(old);
        }
        transaction.commit().map_err(StoreError::redb)?;
        Ok(())
    }

    pub fn replace_key(
        &self,
        table_name: &str,
        old_key: &[u8],
        new_key: &[u8],
        value: &[u8],
    ) -> Result<(), StoreError> {
        validate_key(new_key)?;
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        {
            let mut table = transaction
                .open_table(raw_table(table_name)?)
                .map_err(StoreError::redb)?;
            if old_key != new_key {
                let removed = table.remove(old_key).map_err(StoreError::redb)?;
                drop(removed);
            }
            let replaced = table.insert(new_key, value).map_err(StoreError::redb)?;
            drop(replaced);
        }
        transaction.commit().map_err(StoreError::redb)?;
        Ok(())
    }

    pub fn delete_row(&self, table_name: &str, key: &[u8]) -> Result<bool, StoreError> {
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        let existed = {
            let mut table = transaction
                .open_table(raw_table(table_name)?)
                .map_err(StoreError::redb)?;
            let removed = table.remove(key).map_err(StoreError::redb)?;
            let existed = removed.is_some();
            drop(removed);
            existed
        };
        transaction.commit().map_err(StoreError::redb)?;
        Ok(existed)
    }

    fn seed_demo(&mut self) -> Result<(), StoreError> {
        let transaction = self.db.begin_write().map_err(StoreError::redb)?;
        {
            let mut people = transaction
                .open_table(raw_table("people")?)
                .map_err(StoreError::redb)?;
            drop(
                people
                    .insert(&b"alice"[..], &b"systems"[..])
                    .map_err(StoreError::redb)?,
            );
            drop(
                people
                    .insert(&b"bob"[..], &b"graphics"[..])
                    .map_err(StoreError::redb)?,
            );
            drop(
                people
                    .insert(&b"carol"[..], &b"storage"[..])
                    .map_err(StoreError::redb)?,
            );
        }
        {
            let mut settings = transaction
                .open_table(raw_table("settings")?)
                .map_err(StoreError::redb)?;
            drop(
                settings
                    .insert(&b"theme"[..], &b"terminal"[..])
                    .map_err(StoreError::redb)?,
            );
            drop(
                settings
                    .insert(&b"persistence"[..], &b"off"[..])
                    .map_err(StoreError::redb)?,
            );
            drop(
                settings
                    .insert(&b"engine"[..], &b"redb no_std"[..])
                    .map_err(StoreError::redb)?,
            );
        }
        {
            let mut packets = transaction
                .open_table(raw_table("packets")?)
                .map_err(StoreError::redb)?;
            drop(
                packets
                    .insert(&b"header"[..], &[0_u8, 255, 16, 32][..])
                    .map_err(StoreError::redb)?,
            );
        }
        transaction.commit().map_err(StoreError::redb)?;
        Ok(())
    }
}

fn create_database() -> Result<Database, StoreError> {
    Database::builder()
        .create_with_backend(InMemoryBackend::new())
        .map_err(StoreError::redb)
}

fn raw_table(name: &str) -> Result<RawTable<'_>, StoreError> {
    validate_table_name(name)?;
    Ok(TableDefinition::new(name))
}

fn normalize_table_name(input: &str) -> Result<String, StoreError> {
    let name = input.trim();
    validate_table_name(name)?;
    Ok(name.to_owned())
}

fn validate_table_name(name: &str) -> Result<(), StoreError> {
    if name.is_empty() {
        return Err(StoreError::message("table name cannot be empty"));
    }
    if name.len() > 96 {
        return Err(StoreError::message("table name is limited to 96 bytes"));
    }
    if name.chars().any(char::is_control) {
        return Err(StoreError::message(
            "table name cannot contain control characters",
        ));
    }
    Ok(())
}

fn validate_key(key: &[u8]) -> Result<(), StoreError> {
    if key.is_empty() {
        return Err(StoreError::message("row key cannot be empty"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    fn redb(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }

    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}
