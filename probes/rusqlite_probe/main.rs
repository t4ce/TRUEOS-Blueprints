use core::fmt;

use rusqlite_fork::{Connection, MAIN_DB, params};
use trueos::{async_fs, logl, logl::level, platform};

const COMMON_DIR: &str = "/common";
const DB_PATH: &str = "/common/usersettings.db";

#[derive(Debug)]
enum ProbeError {
    Sqlite(rusqlite_fork::Error),
    Vfs(i32),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite: {err}"),
            Self::Vfs(rc) => write!(f, "vfs rc={rc}"),
        }
    }
}

impl From<rusqlite_fork::Error> for ProbeError {
    fn from(err: rusqlite_fork::Error) -> Self {
        Self::Sqlite(err)
    }
}

struct Database {
    conn: Connection,
    loaded_bytes: usize,
}

struct UserSettings {
    user_id: i64,
    nickname: String,
    settings_id: i64,
    language: String,
}

struct ProbeReport {
    sqlite_version: String,
    existed_before_open: bool,
    loaded_bytes: usize,
    persisted_bytes: usize,
    user_count: i64,
    settings_count: i64,
    sample: UserSettings,
}

impl Database {
    fn open() -> Result<Self, ProbeError> {
        async_fs::block_on(async_fs::create_dir_all(COMMON_DIR.as_bytes()))
            .map_err(ProbeError::Vfs)?;
        let mut conn = Connection::open_in_memory()?;
        let mut loaded_bytes = 0;

        if async_fs::block_on(async_fs::exists(DB_PATH.as_bytes())).unwrap_or(false) {
            let bytes = async_fs::block_on(async_fs::read_file(DB_PATH.as_bytes()))
                .map_err(ProbeError::Vfs)?;
            loaded_bytes = bytes.len();
            if !bytes.is_empty() {
                conn.deserialize_read_exact(MAIN_DB, bytes.as_slice(), bytes.len(), false)?;
            }
        }

        Ok(Self { conn, loaded_bytes })
    }

    fn sqlite_version(&self) -> Result<String, ProbeError> {
        Ok(self
            .conn
            .query_row("select sqlite_version()", [], |row| row.get(0))?)
    }

    fn migrate(&self) -> Result<(), ProbeError> {
        self.conn.execute_batch(
            "pragma foreign_keys = on;
            create table if not exists \"user\" (
            id integer primary key,
            nickname text not null
        );
            create table if not exists settings (
            user_id integer not null,
            settings_id integer primary key,
            language text not null,
            foreign key (user_id) references \"user\"(id)
        );
            pragma user_version = 1;",
        )?;
        Ok(())
    }

    fn seed_sample(&self) -> Result<(), ProbeError> {
        self.conn.execute(
            "insert into \"user\" (id, nickname)
            values (?1, ?2)
            on conflict(id) do update set nickname = excluded.nickname",
            params![1_i64, "blueprint-user"],
        )?;
        self.conn.execute(
            "insert into settings (user_id, settings_id, language)
            values (?1, ?2, ?3)
            on conflict(settings_id) do update set
                user_id = excluded.user_id,
                language = excluded.language",
            params![1_i64, 1_i64, "en"],
        )?;
        Ok(())
    }

    fn load_sample(&self) -> Result<UserSettings, ProbeError> {
        Ok(self.conn.query_row(
            "select \"user\".id, \"user\".nickname, settings.settings_id, settings.language
            from \"user\"
            join settings on settings.user_id = \"user\".id
            where \"user\".id = ?1 and settings.settings_id = ?2",
            params![1_i64, 1_i64],
            |row| {
                Ok(UserSettings {
                    user_id: row.get(0)?,
                    nickname: row.get(1)?,
                    settings_id: row.get(2)?,
                    language: row.get(3)?,
                })
            },
        )?)
    }

    fn user_count(&self) -> Result<i64, ProbeError> {
        Ok(self
            .conn
            .query_row("select count(*) from \"user\"", [], |row| row.get(0))?)
    }

    fn settings_count(&self) -> Result<i64, ProbeError> {
        Ok(self
            .conn
            .query_row("select count(*) from settings", [], |row| row.get(0))?)
    }

    fn persist(&self) -> Result<usize, ProbeError> {
        let data = self.conn.serialize(MAIN_DB)?;
        let len = data.len();
        async_fs::block_on(async_fs::write_file(DB_PATH.as_bytes(), &data))
            .map_err(ProbeError::Vfs)?;
        Ok(len)
    }
}

fn run_probe() -> Result<ProbeReport, ProbeError> {
    let existed_before_open =
        async_fs::block_on(async_fs::exists(DB_PATH.as_bytes())).unwrap_or(false);
    let db = Database::open()?;
    let sqlite_version = db.sqlite_version()?;

    db.migrate()?;
    db.seed_sample()?;
    let sample = db.load_sample()?;
    let persisted_bytes = db.persist()?;

    Ok(ProbeReport {
        sqlite_version,
        existed_before_open,
        loaded_bytes: db.loaded_bytes,
        persisted_bytes,
        user_count: db.user_count()?,
        settings_count: db.settings_count()?,
        sample,
    })
}

fn log_report(report: ProbeReport) {
    logl::log(
        level::INFO,
        format_args!(
            "rusqlite-probe: sqlite_version={} db_path={} mode=memory-serialize existed_before_open={} loaded_bytes={} persisted_bytes={} users={} settings={} user_id={} nickname={} settings_id={} language={}",
            report.sqlite_version,
            DB_PATH,
            report.existed_before_open,
            report.loaded_bytes,
            report.persisted_bytes,
            report.user_count,
            report.settings_count,
            report.sample.user_id,
            report.sample.nickname,
            report.sample.settings_id,
            report.sample.language,
        ),
    );
}

fn main() {
    logl::log(level::INFO, "rusqlite-probe: blueprint start");
    match run_probe() {
        Ok(report) => {
            log_report(report);
            logl::log(level::INFO, "rusqlite-probe: ok");
        }
        Err(err) => logl::log(level::ERROR, format_args!("rusqlite-probe: failed {}", err)),
    }
    platform::poll_once();
}
