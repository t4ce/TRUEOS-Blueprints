//! Transactional RAM storage owned by a server instance. Blueprint lifecycle
//! management owns snapshots and replication; this backend never opens a file.
use super::{ConnectionMode, DatabaseSettings, SqlLogMode, error::PersistenceError};
use redb::{
    Database, ReadTransaction, ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    ops::Deref,
    sync::{Arc, RwLock},
};

pub(super) type CrudTable = TableDefinition<'static, &'static str, &'static [u8]>;
pub(super) const CHARACTERS: CrudTable = TableDefinition::new("characters");
pub(super) const PLAYERS: CrudTable = TableDefinition::new("players");
pub(super) const ITEMS: CrudTable = TableDefinition::new("items");
pub(super) const META: CrudTable = TableDefinition::new("metadata");

pub(super) fn storage_error(error: impl std::fmt::Display) -> PersistenceError {
    PersistenceError::OtherError(format!("redb: {error}"))
}

/// Reads always use a transaction, so a character and its indexes are observed
/// at the same committed revision. Writers can read their own pending changes.
pub(super) trait CrudRead {
    fn get<T: DeserializeOwned>(
        &self,
        table: CrudTable,
        key: &str,
    ) -> Result<Option<T>, PersistenceError>;
}

macro_rules! impl_read {
    ($transaction:ty) => {
        impl CrudRead for $transaction {
            fn get<T: DeserializeOwned>(
                &self,
                definition: CrudTable,
                key: &str,
            ) -> Result<Option<T>, PersistenceError> {
                let table = self.open_table(definition).map_err(storage_error)?;
                let value = table.get(key).map_err(storage_error)?;
                value
                    .map(|value| serde_json::from_slice(value.value()).map_err(Into::into))
                    .transpose()
            }
        }
    };
}
impl_read!(ReadTransaction);
impl_read!(WriteTransaction);

/// Explicit CRUD rather than SQL strings or implicit upserts.
pub(super) trait CrudWrite: CrudRead {
    fn create<T: Serialize>(
        &self,
        table: CrudTable,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError>;
    fn update<T: Serialize>(
        &self,
        table: CrudTable,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError>;
    fn delete(&self, table: CrudTable, key: &str) -> Result<bool, PersistenceError>;
}

impl CrudWrite for WriteTransaction {
    fn create<T: Serialize>(
        &self,
        definition: CrudTable,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError> {
        let bytes = serde_json::to_vec(value)?;
        let mut table = self.open_table(definition).map_err(storage_error)?;
        if table.get(key).map_err(storage_error)?.is_some() {
            return Err(storage_error(format!("record already exists: {key}")));
        }
        table.insert(key, bytes.as_slice()).map_err(storage_error)?;
        Ok(())
    }

    fn update<T: Serialize>(
        &self,
        definition: CrudTable,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError> {
        let bytes = serde_json::to_vec(value)?;
        let mut table = self.open_table(definition).map_err(storage_error)?;
        if table.get(key).map_err(storage_error)?.is_none() {
            return Err(storage_error(format!("record does not exist: {key}")));
        }
        table.insert(key, bytes.as_slice()).map_err(storage_error)?;
        Ok(())
    }

    fn delete(&self, definition: CrudTable, key: &str) -> Result<bool, PersistenceError> {
        let mut table = self.open_table(definition).map_err(storage_error)?;
        let removed = table.remove(key).map_err(storage_error)?.is_some();
        Ok(removed)
    }
}

pub(crate) struct Connection {
    database: Arc<Database>,
    mode: ConnectionMode,
}

impl Connection {
    pub(super) fn read(&self) -> Result<ReadTransaction, PersistenceError> {
        self.database.begin_read().map_err(storage_error)
    }

    pub(super) fn transaction(&mut self) -> Result<WriteTransaction, PersistenceError> {
        if self.mode != ConnectionMode::ReadWrite {
            return Err(storage_error("cannot write through a read-only connection"));
        }
        self.database.begin_write().map_err(storage_error)
    }
}

pub(crate) struct VelorenConnection {
    pub(super) connection: Connection,
    sql_log_mode: SqlLogMode,
}

impl VelorenConnection {
    pub fn update_log_mode(&mut self, settings: &Arc<RwLock<DatabaseSettings>>) {
        let mode = settings
            .read()
            .expect("DatabaseSettings RwLock was poisoned")
            .sql_log_mode;
        if self.sql_log_mode != mode {
            self.sql_log_mode = mode;
            tracing::info!(
                ?mode,
                "SQL statement logging is unavailable for redb storage"
            );
        }
    }
}

impl Deref for VelorenConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection { &self.connection }
}

fn database(settings: &DatabaseSettings) -> Arc<Database> {
    Arc::clone(settings.database.get_or_init(|| {
        let database = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .expect("Failed to initialize server redb RAM database");
        let write = database
            .begin_write()
            .expect("Failed to initialize redb schema");
        for table in [CHARACTERS, PLAYERS, ITEMS, META] {
            drop(
                write
                    .open_table(table)
                    .expect("Failed to initialize redb table"),
            );
        }
        write
            .create(META, "schema_version", &1_u32)
            .expect("Failed to initialize redb schema version");
        // ID 1 is reserved for the world pseudo-container in existing item conversions.
        write
            .create(META, "next_entity_id", &2_i64)
            .expect("Failed to initialize redb entity IDs");
        write.commit().expect("Failed to commit redb schema");
        Arc::new(database)
    }))
}

pub(crate) fn establish_connection(
    settings: &DatabaseSettings,
    mode: ConnectionMode,
) -> VelorenConnection {
    VelorenConnection {
        connection: Connection {
            database: database(settings),
            mode,
        },
        sql_log_mode: settings.sql_log_mode,
    }
}

/// Startup compatibility entry point: initializes the RAM schema, without SQL
/// migrations.
pub fn run_migrations(settings: &DatabaseSettings) {
    let _ = database(settings);
    tracing::info!("Character storage: redb RAM database owned by this server instance");
}

/// No filesystem vacuum is necessary for a newly initialized RAM database.
pub fn vacuum_database(_settings: &DatabaseSettings) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn settings() -> DatabaseSettings {
        DatabaseSettings::new(PathBuf::from("unused"), SqlLogMode::Disabled)
    }

    #[test]
    fn crud_is_atomic_and_readers_keep_their_snapshot() {
        let settings = settings();
        let mut writer = establish_connection(&settings, ConnectionMode::ReadWrite);
        let reader = establish_connection(&settings.clone(), ConnectionMode::ReadOnly);
        let old = reader.read().unwrap();
        let transaction = writer.connection.transaction().unwrap();
        transaction.create(CHARACTERS, "2", &"first").unwrap();
        transaction.create(PLAYERS, "owner", &vec![2_i64]).unwrap();
        assert!(
            reader
                .read()
                .unwrap()
                .get::<String>(CHARACTERS, "2")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            transaction
                .get::<String>(CHARACTERS, "2")
                .unwrap()
                .as_deref(),
            Some("first")
        );
        transaction.commit().unwrap();
        assert!(old.get::<String>(CHARACTERS, "2").unwrap().is_none());
        let committed = reader.read().unwrap();
        assert_eq!(
            committed.get::<Vec<i64>>(PLAYERS, "owner").unwrap(),
            Some(vec![2])
        );
        assert_eq!(
            committed.get::<String>(CHARACTERS, "2").unwrap().as_deref(),
            Some("first")
        );
        let transaction = writer.connection.transaction().unwrap();
        transaction.update(CHARACTERS, "2", &"second").unwrap();
        assert!(transaction.create(CHARACTERS, "2", &"duplicate").is_err());
        assert!(transaction.update(CHARACTERS, "missing", &"value").is_err());
        drop(transaction); // rollback
        assert_eq!(
            reader
                .read()
                .unwrap()
                .get::<String>(CHARACTERS, "2")
                .unwrap()
                .as_deref(),
            Some("first")
        );
        let transaction = writer.connection.transaction().unwrap();
        assert!(transaction.delete(CHARACTERS, "2").unwrap());
        transaction.delete(PLAYERS, "owner").unwrap();
        transaction.commit().unwrap();
        assert!(
            reader
                .read()
                .unwrap()
                .get::<String>(CHARACTERS, "2")
                .unwrap()
                .is_none()
        );
        assert!(
            reader
                .read()
                .unwrap()
                .get::<Vec<i64>>(PLAYERS, "owner")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn instances_are_isolated_and_read_only_connections_cannot_write() {
        let first = settings();
        let mut writer = establish_connection(&first, ConnectionMode::ReadWrite);
        let transaction = writer.connection.transaction().unwrap();
        transaction
            .create(CHARACTERS, "2", &"first instance")
            .unwrap();
        transaction.commit().unwrap();
        let mut other = establish_connection(&settings(), ConnectionMode::ReadOnly);
        assert!(
            other
                .read()
                .unwrap()
                .get::<String>(CHARACTERS, "2")
                .unwrap()
                .is_none()
        );
        assert!(other.connection.transaction().is_err());
    }
}
