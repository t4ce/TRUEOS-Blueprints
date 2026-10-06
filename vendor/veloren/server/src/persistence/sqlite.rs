//! SQLite backend for native Veloren builds.
use super::*;
use refinery::Report;
use rusqlite::{
    OpenFlags,
    trace::{TraceEvent, TraceEventCodes},
};
use std::{
    fs,
    ops::Deref,
    sync::{Arc, RwLock},
};
use tracing::info;
#[path = "diesel_to_rusqlite.rs"]
mod diesel_to_rusqlite;
mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("./src/migrations");
}
/// A database connection blessed by Veloren.
pub(crate) struct VelorenConnection {
    pub(super) connection: Connection,
    sql_log_mode: SqlLogMode,
}

impl VelorenConnection {
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            sql_log_mode: SqlLogMode::Disabled,
        }
    }

    /// Updates the SQLite log mode if DatabaseSetting.sql_log_mode has changed
    pub fn update_log_mode(&mut self, database_settings: &Arc<RwLock<DatabaseSettings>>) {
        let settings = database_settings
            .read()
            .expect("DatabaseSettings RwLock was poisoned");
        if self.sql_log_mode == settings.sql_log_mode {
            return;
        }

        set_log_mode(&mut self.connection, settings.sql_log_mode);
        self.sql_log_mode = settings.sql_log_mode;

        info!(
            "SQL log mode for connection changed to {:?}",
            settings.sql_log_mode
        );
    }
}

impl Deref for VelorenConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection { &self.connection }
}

fn set_log_mode(connection: &mut Connection, sql_log_mode: SqlLogMode) {
    match sql_log_mode {
        SqlLogMode::Trace => {
            connection.trace_v2(
                TraceEventCodes::SQLITE_TRACE_STMT,
                Some(rusqlite_trace_callback),
            );
        },
        SqlLogMode::Profile => {
            connection.trace_v2(
                TraceEventCodes::SQLITE_TRACE_PROFILE,
                Some(rusqlite_trace_callback),
            );
        },
        SqlLogMode::Disabled => {
            connection.trace_v2(TraceEventCodes::empty(), None);
        },
    };
}

/// Runs any pending database migrations. This is executed during server startup
pub fn run_migrations(settings: &DatabaseSettings) {
    let mut conn = establish_connection(settings, ConnectionMode::ReadWrite);

    diesel_to_rusqlite::migrate_from_diesel(&mut conn)
        .expect("One-time migration from Diesel to Refinery failed");

    // If migrations fail to run, the server cannot start since the database will
    // not be in the required state.
    let report: Report = embedded::migrations::runner()
        .set_abort_divergent(false)
        .run(&mut conn.connection)
        .expect("Database migrations failed, server startup aborted");

    let applied_migrations = report.applied_migrations().len();
    info!("Applied {} database migrations", applied_migrations);
}

/// Runs after the migrations. In some cases, it can reclaim a significant
/// amount of space (reported 30%)
pub fn vacuum_database(settings: &DatabaseSettings) {
    let conn = establish_connection(settings, ConnectionMode::ReadWrite);

    conn.execute("VACUUM main", [])
        .expect("Database vacuuming failed, server startup aborted");

    info!("Database vacuumed");
}

// This callback uses info logging because it is never enabled by default,
// only when explicitly turned on via CLI arguments or interactive CLI commands.
// Setting it to anything other than info would remove the ability to get SQL
// logging from a running server that wasn't started at higher than info.
fn rusqlite_trace_callback(event: TraceEvent<'_>) {
    match event {
        TraceEvent::Stmt(_, msg) => info!("{}", msg),
        TraceEvent::Profile(stmt, dur) => info!("{} Duration: {:?}", stmt.sql(), dur),
        _ => (),
    }
}

pub(crate) fn establish_connection(
    settings: &DatabaseSettings,
    connection_mode: ConnectionMode,
) -> VelorenConnection {
    fs::create_dir_all(&settings.db_dir)
        .unwrap_or_else(|_| panic!("Failed to create saves directory: {:?}", settings.db_dir));

    let open_flags = OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | match connection_mode {
            ConnectionMode::ReadWrite => {
                OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE
            },
            ConnectionMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
        };

    let connection = Connection::open_with_flags(settings.db_dir.join("db.sqlite"), open_flags)
        .unwrap_or_else(|err| {
            panic!(
                "Error connecting to {}, Error: {:?}",
                settings.db_dir.join("db.sqlite").display(),
                err
            )
        });

    let mut veloren_connection = VelorenConnection::new(connection);

    let connection = &mut veloren_connection.connection;

    set_log_mode(connection, settings.sql_log_mode);
    veloren_connection.sql_log_mode = settings.sql_log_mode;

    rusqlite::vtab::array::load_module(connection).expect("Failed to load sqlite array module");

    connection.set_prepared_statement_cache_capacity(100);

    // Use Write-Ahead-Logging for improved concurrency: https://sqlite.org/wal.html
    // Set a busy timeout (in ms): https://sqlite.org/c3ref/busy_timeout.html
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("Failed to set foreign_keys PRAGMA");
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .expect("Failed to set journal_mode PRAGMA");
    connection
        .pragma_update(None, "busy_timeout", "250")
        .expect("Failed to set busy_timeout PRAGMA");

    veloren_connection
}
