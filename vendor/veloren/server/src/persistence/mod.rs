//! Character storage and worker interfaces. TRUEOS Blueprints use transactional
//! redb RAM storage; native builds retain their SQLite save compatibility.

#[cfg(not(feature = "redb-backend"))]
pub(in crate::persistence) mod character;
#[cfg(feature = "redb-backend")]
#[path = "character/redb.rs"]
pub(in crate::persistence) mod character;
#[cfg(not(feature = "redb-backend"))] mod sqlite;
#[cfg(not(feature = "redb-backend"))]
pub(crate) use rusqlite::Connection;
#[cfg(not(feature = "redb-backend"))]
pub(crate) use sqlite::{VelorenConnection, establish_connection};
#[cfg(not(feature = "redb-backend"))]
pub use sqlite::{run_migrations, vacuum_database};
#[cfg(feature = "redb-backend")] mod redb_store;
#[cfg(feature = "redb-backend")]
pub(crate) use redb_store::{Connection, VelorenConnection, establish_connection};
#[cfg(feature = "redb-backend")]
pub use redb_store::{run_migrations, vacuum_database};
pub mod character_loader;
pub mod character_updater;
pub mod error;
mod json_models;
mod models;

use crate::persistence::character_updater::PetPersistenceData;
use common::comp;
use std::path::PathBuf;
#[cfg(feature = "redb-backend")]
use std::sync::Arc;

/// A struct of the components that are persisted to the DB for each character
#[derive(Debug)]
pub struct PersistedComponents {
    pub body: comp::Body,
    pub hardcore: Option<comp::Hardcore>,
    pub stats: comp::Stats,
    pub skill_set: comp::SkillSet,
    pub inventory: comp::Inventory,
    pub waypoint: Option<comp::Waypoint>,
    pub pets: Vec<PetPersistenceData>,
    pub active_abilities: comp::ActiveAbilities,
    pub map_marker: Option<comp::MapMarker>,
}

pub type EditableComponents = (comp::Body,);

#[derive(Clone)]
pub struct DatabaseSettings {
    pub db_dir: PathBuf,
    pub sql_log_mode: SqlLogMode,
    #[cfg(feature = "redb-backend")]
    database: Arc<std::sync::OnceLock<Arc<redb::Database>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SqlLogMode {
    /// Logging is disabled
    #[default]
    Disabled,
    /// Records timings for each SQL statement
    Profile,
    /// Prints all executed SQL statements
    Trace,
}

impl SqlLogMode {
    pub fn variants() -> [&'static str; 3] { ["disabled", "profile", "trace"] }
}

impl core::str::FromStr for SqlLogMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "disabled" => Ok(Self::Disabled),
            "profile" => Ok(Self::Profile),
            "trace" => Ok(Self::Trace),
            _ => Err("Could not parse SqlLogMode"),
        }
    }
}

#[expect(clippy::to_string_trait_impl)]
impl ToString for SqlLogMode {
    fn to_string(&self) -> String {
        match self {
            SqlLogMode::Disabled => "disabled",
            SqlLogMode::Profile => "profile",
            SqlLogMode::Trace => "trace",
        }
        .into()
    }
}

impl DatabaseSettings {
    /// Each settings instance owns a separate database; clones share that
    /// instance.
    pub fn new(db_dir: PathBuf, sql_log_mode: SqlLogMode) -> Self {
        Self {
            db_dir,
            sql_log_mode,
            #[cfg(feature = "redb-backend")]
            database: Arc::new(std::sync::OnceLock::new()),
        }
    }
}
