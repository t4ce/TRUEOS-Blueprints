use core::fmt;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite_fork::{Connection, MAIN_DB, params};
use trueos::{async_fs, logl, logl::level, platform, t};

const COMMON_DIR: &str = "/common";
const DB_PATH: &str = "/common/usersettings-multirt.db";
// Two explicit native runtime lanes; never infer capacity from std threads.
const WORKERS: usize = 2;
const BATCHES_PER_WORKER: usize = 16;
const ROWS_PER_BATCH: usize = 16;
const OPS_PER_WORKER: usize = BATCHES_PER_WORKER * ROWS_PER_BATCH;
const START_WAIT_MS: usize = 2_000;

#[derive(Debug)]
enum ProbeError {
    Sqlite(rusqlite_fork::Error),
    Vfs(i32),
    Executor(&'static str),
    Invariant(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "sqlite: {error}"),
            Self::Vfs(rc) => write!(f, "vfs rc={rc}"),
            Self::Executor(stage) => write!(f, "executor: {stage}"),
            Self::Invariant(message) => write!(f, "invariant: {message}"),
        }
    }
}

impl From<rusqlite_fork::Error> for ProbeError {
    fn from(error: rusqlite_fork::Error) -> Self {
        Self::Sqlite(error)
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

struct ColdReport {
    sqlite_version: String,
    threadsafe: bool,
    existed_before_bootstrap: bool,
    loaded_bytes: usize,
    persisted_bytes: usize,
    user_count: i64,
    settings_count: i64,
    sample: UserSettings,
    image: Vec<u8>,
}

struct WorkerReport {
    worker: usize,
    wls_slot: usize,
    operations: usize,
    checksum: i64,
    image_bytes: usize,
    image_digest: u64,
}

#[derive(Copy, Clone)]
struct Summary {
    workers: usize,
    operations: usize,
    checksum: i64,
    distinct_slots: usize,
    max_active: usize,
    worker_image_bytes: usize,
    digest: u64,
}

struct MultiRtReport {
    cold: ColdReport,
    summary: Summary,
    ready_before_release: usize,
    final_persisted_bytes: usize,
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

    fn threadsafe(&self) -> Result<bool, ProbeError> {
        let enabled: i64 = self.conn.query_row(
            "select sqlite_compileoption_used('THREADSAFE=1')",
            [],
            |row| row.get(0),
        )?;
        Ok(enabled == 1)
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
             pragma user_version = 2;",
        )?;
        Ok(())
    }

    fn seed_sample(&self) -> Result<(), ProbeError> {
        self.conn.execute(
            "insert into \"user\" (id, nickname)
             values (?1, ?2)
             on conflict(id) do update set nickname = excluded.nickname",
            params![1_i64, "multirt-user"],
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

    fn count(&self, table: &str) -> Result<i64, ProbeError> {
        let sql = format!("select count(*) from {table}");
        Ok(self.conn.query_row(&sql, [], |row| row.get(0))?)
    }

    fn serialize(&self) -> Result<Vec<u8>, ProbeError> {
        let data = self.conn.serialize(MAIN_DB)?;
        Ok(data.to_vec())
    }

    fn persist(&self) -> Result<Vec<u8>, ProbeError> {
        let image = self.serialize()?;
        async_fs::block_on(async_fs::write_file(DB_PATH.as_bytes(), &image))
            .map_err(ProbeError::Vfs)?;
        Ok(image)
    }
}

fn run_cold_read() -> Result<ColdReport, ProbeError> {
    let existed_before_bootstrap =
        async_fs::block_on(async_fs::exists(DB_PATH.as_bytes())).unwrap_or(false);

    // Bootstrap first so even a pristine VM performs a genuine persisted
    // cold read below rather than only exercising an empty in-memory open.
    let bootstrap = Database::open()?;
    bootstrap.migrate()?;
    bootstrap.seed_sample()?;
    let persisted = bootstrap.persist()?;
    drop(bootstrap);

    let cold = Database::open()?;
    let sqlite_version = cold.sqlite_version()?;
    let threadsafe = cold.threadsafe()?;
    let sample = cold.load_sample()?;
    let image = cold.serialize()?;
    if cold.loaded_bytes != persisted.len() || image.len() != persisted.len() {
        return Err(ProbeError::Invariant(format!(
            "cold image size mismatch written={} loaded={} serialized={}",
            persisted.len(),
            cold.loaded_bytes,
            image.len()
        )));
    }
    if !threadsafe {
        return Err(ProbeError::Invariant(
            "bundled SQLite did not report THREADSAFE=1".to_string(),
        ));
    }

    Ok(ColdReport {
        sqlite_version,
        threadsafe,
        existed_before_bootstrap,
        loaded_bytes: cold.loaded_bytes,
        persisted_bytes: persisted.len(),
        user_count: cold.count("\"user\"")?,
        settings_count: cold.count("settings")?,
        sample,
        image,
    })
}

async fn run_worker(
    worker: usize,
    image: Arc<Vec<u8>>,
    ready: Arc<AtomicUsize>,
    release: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
) -> Result<WorkerReport, ProbeError> {
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact(MAIN_DB, image.as_slice(), image.len(), false)?;
    conn.execute_batch(
        "create table if not exists multirt_event (
             worker_id integer not null,
             seq integer not null,
             payload integer not null,
             primary key (worker_id, seq)
         ) without rowid;
         delete from multirt_event;",
    )?;

    let wls_slot = t::worker::local_slot() as usize;
    ready.fetch_add(1, Ordering::AcqRel);
    t::time::timeout(
        t::time::Duration::from_millis(START_WAIT_MS as u64),
        async {
            while release.load(Ordering::Acquire) == 0 {
                t::time::sleep(t::time::Duration::from_millis(1)).await;
            }
        },
    )
    .await
    .map_err(|_| ProbeError::Executor("worker release timeout"))?;
    if release.load(Ordering::Acquire) == 2 {
        return Err(ProbeError::Executor("fleet cancelled"));
    }

    let now_active = active.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    max_active.fetch_max(now_active, Ordering::AcqRel);
    let result = run_worker_transactions(worker, wls_slot, &mut conn).await;
    active.fetch_sub(1, Ordering::AcqRel);
    result
}

async fn run_worker_transactions(
    worker: usize,
    wls_slot: usize,
    conn: &mut Connection,
) -> Result<WorkerReport, ProbeError> {
    let mut expected_checksum = 0i64;
    for batch in 0..BATCHES_PER_WORKER {
        let tx = conn.transaction()?;
        for offset in 0..ROWS_PER_BATCH {
            let seq = batch * ROWS_PER_BATCH + offset;
            let payload = (worker as i64 + 1) * 1_000_000 + seq as i64;
            tx.execute(
                "insert into multirt_event (worker_id, seq, payload)
                 values (?1, ?2, ?3)
                 on conflict(worker_id, seq) do update set payload = excluded.payload",
                params![worker as i64, seq as i64, payload],
            )?;
            expected_checksum = expected_checksum.wrapping_add(payload);
        }
        let observed: i64 = tx.query_row(
            "select count(*) from multirt_event where worker_id = ?1",
            params![worker as i64],
            |row| row.get(0),
        )?;
        let expected = ((batch + 1) * ROWS_PER_BATCH) as i64;
        if observed != expected {
            return Err(ProbeError::Invariant(format!(
                "worker={worker} batch={batch} rows={observed} expected={expected}"
            )));
        }
        tx.commit()?;
        t::task::yield_now().await;
        t::time::sleep(t::time::Duration::from_millis(1)).await;
        if t::worker::local_slot() as usize != wls_slot {
            return Err(ProbeError::Executor("worker WLS moved"));
        }
    }

    let (rows, checksum): (i64, i64) = conn.query_row(
        "select count(*), coalesce(sum(payload), 0)
         from multirt_event where worker_id = ?1",
        params![worker as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if rows != OPS_PER_WORKER as i64 || checksum != expected_checksum {
        return Err(ProbeError::Invariant(format!(
            "worker={worker} final rows={rows} checksum={checksum} expected_rows={} expected_checksum={expected_checksum}",
            OPS_PER_WORKER
        )));
    }

    let image = conn.serialize(MAIN_DB)?;
    // Reopen the serialized image and verify each row, not just its size/digest.
    let mut restored = Connection::open_in_memory()?;
    restored.deserialize_read_exact(MAIN_DB, &image, image.len(), false)?;
    let mut statement = restored
        .prepare("select seq, payload from multirt_event where worker_id = ?1 order by seq")?;
    let mut records = statement.query(params![worker as i64])?;
    for seq in 0..OPS_PER_WORKER {
        let row = records
            .next()?
            .ok_or_else(|| ProbeError::Invariant("serialized row missing".into()))?;
        if row.get::<_, i64>(0)? != seq as i64
            || row.get::<_, i64>(1)? != (worker as i64 + 1) * 1_000_000 + seq as i64
        {
            return Err(ProbeError::Invariant("serialized row mismatch".into()));
        }
    }
    if records.next()?.is_some() {
        return Err(ProbeError::Invariant("extra serialized row".into()));
    }
    Ok(WorkerReport {
        worker,
        wls_slot,
        operations: OPS_PER_WORKER,
        checksum,
        image_bytes: image.len(),
        image_digest: digest(&image),
    })
}

fn persist_summary(image: Arc<Vec<u8>>, summary: Summary) -> Result<usize, ProbeError> {
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact(MAIN_DB, image.as_slice(), image.len(), false)?;
    conn.execute_batch(
        "create table if not exists multirt_summary (
             id integer primary key check (id = 1),
             workers integer not null,
             operations integer not null,
             checksum integer not null,
             distinct_slots integer not null,
             max_active integer not null,
             worker_image_bytes integer not null,
             digest integer not null
         );",
    )?;
    conn.execute(
        "insert into multirt_summary
             (id, workers, operations, checksum, distinct_slots, max_active, worker_image_bytes, digest)
         values (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         on conflict(id) do update set
             workers = excluded.workers,
             operations = excluded.operations,
             checksum = excluded.checksum,
             distinct_slots = excluded.distinct_slots,
             max_active = excluded.max_active,
             worker_image_bytes = excluded.worker_image_bytes,
             digest = excluded.digest",
        params![
            summary.workers as i64,
            summary.operations as i64,
            summary.checksum,
            summary.distinct_slots as i64,
            summary.max_active as i64,
            summary.worker_image_bytes as i64,
            summary.digest as i64,
        ],
    )?;
    let data = conn.serialize(MAIN_DB)?;
    let bytes = data.len();
    async_fs::block_on(async_fs::write_file(DB_PATH.as_bytes(), &data)).map_err(ProbeError::Vfs)?;
    Ok(bytes)
}

async fn join_native<T>(
    mut job: t::worker::JoinHandle<Result<T, ProbeError>>,
    stage: &'static str,
) -> Result<T, ProbeError> {
    match t::time::timeout(t::time::Duration::from_secs(10), &mut job).await {
        Ok(result) => result.map_err(|_| ProbeError::Executor(stage))?,
        Err(_) => {
            logl::log(
                level::ERROR,
                format_args!("rusqlite-multirt: FAIL stage={stage} timeout=true action=draining"),
            );
            let _ = job.await;
            Err(ProbeError::Executor(stage))
        }
    }
}

async fn run_multirt() -> Result<MultiRtReport, ProbeError> {
    logl::log(
        level::INFO,
        format_args!("rusqlite-multirt: phase=cold-read executor=native-worker begin"),
    );
    let cold = join_native(
        t::worker::spawn(run_cold_read).map_err(|_| ProbeError::Executor("cold-read submit"))?,
        "cold-read join",
    )
    .await?;
    logl::log(
        level::INFO,
        format_args!(
            "rusqlite-multirt: phase=cold-read status=ok existed={} loaded_bytes={} persisted_bytes={} threadsafe={}",
            cold.existed_before_bootstrap,
            cold.loaded_bytes,
            cold.persisted_bytes,
            cold.threadsafe as u8,
        ),
    );

    // TRUEOS persistence currently stores a whole serialized SQLite image, not
    // a shared SQLite VFS. Give each lane its own Connection and snapshot: this
    // proves concurrent SQLite/runtime execution without claiming WAL or
    // shared-file locking semantics that the platform does not expose yet.
    let image = Arc::new(cold.image.clone());
    let ready = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    let mut failure = None;
    t::time::timeout(
        t::time::Duration::from_millis(START_WAIT_MS as u64),
        async {
            while t::worker::capacity() < WORKERS {
                t::time::sleep(t::time::Duration::from_millis(1)).await;
            }
        },
    )
    .await
    .map_err(|_| ProbeError::Executor("insufficient native capacity"))?;

    logl::log(
        level::INFO,
        format_args!(
            "rusqlite-multirt: phase=pressure executor=native-worker workers={} batches_per_worker={} rows_per_batch={} begin",
            WORKERS, BATCHES_PER_WORKER, ROWS_PER_BATCH
        ),
    );
    for worker in 0..WORKERS {
        let image = Arc::clone(&image);
        let ready = Arc::clone(&ready);
        let release = Arc::clone(&release);
        let active = Arc::clone(&active);
        let max_active = Arc::clone(&max_active);
        match t::worker::spawn(move || {
            let runtime = t::runtime::current_thread()
                .build()
                .map_err(|_| ProbeError::Executor("worker runtime"))?;
            let result = runtime.block_on(run_worker(
                worker, image, ready, release, active, max_active,
            ));
            drop(runtime);
            result
        }) {
            Ok(job) => workers.push(job),
            Err(_) => {
                failure = Some(ProbeError::Executor("worker submit"));
                break;
            }
        }
    }

    let mut waited_ms = 0usize;
    while failure.is_none() && ready.load(Ordering::Acquire) < WORKERS && waited_ms < START_WAIT_MS
    {
        waited_ms = waited_ms.saturating_add(1);
        t::time::sleep(t::time::Duration::from_millis(1)).await;
    }
    let ready_before_release = ready.load(Ordering::Acquire);
    if ready_before_release != WORKERS {
        failure.get_or_insert(ProbeError::Executor("worker ready timeout"));
    }
    release.store(if failure.is_some() { 2 } else { 1 }, Ordering::Release);

    let mut reports = Vec::with_capacity(WORKERS);
    for worker in workers {
        match join_native(worker, "worker join").await {
            Ok(report) => reports.push(report),
            Err(error) => {
                failure.get_or_insert(error);
            }
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    reports.sort_by_key(|report| report.worker);
    let distinct_slots = reports
        .iter()
        .map(|report| report.wls_slot)
        .collect::<BTreeSet<_>>()
        .len();
    let max_active = max_active.load(Ordering::Acquire);
    if ready_before_release != WORKERS || reports.len() != WORKERS || distinct_slots != WORKERS {
        return Err(ProbeError::Invariant(format!(
            "native fleet did not become concurrent ready={} joined={} max_active={max_active} distinct_slots={distinct_slots} expected={} wait_ms={waited_ms}",
            ready_before_release,
            reports.len(),
            WORKERS,
        )));
    }

    let operations = reports
        .iter()
        .map(|report| report.operations)
        .sum::<usize>();
    let checksum = reports
        .iter()
        .fold(0i64, |sum, report| sum.wrapping_add(report.checksum));
    let worker_image_bytes = reports
        .iter()
        .map(|report| report.image_bytes)
        .sum::<usize>();
    let digest = reports.iter().fold(0u64, |value, report| {
        value ^ report.image_digest.rotate_left((report.worker & 63) as u32)
    });
    if operations != WORKERS * OPS_PER_WORKER {
        return Err(ProbeError::Invariant(format!(
            "operation total={operations} expected={}",
            WORKERS * OPS_PER_WORKER
        )));
    }

    let summary = Summary {
        workers: WORKERS,
        operations,
        checksum,
        distinct_slots,
        max_active,
        worker_image_bytes,
        digest,
    };
    let final_image = Arc::clone(&image);
    let final_persisted_bytes = join_native(
        t::worker::spawn(move || persist_summary(final_image, summary))
            .map_err(|_| ProbeError::Executor("summary persistence submit"))?,
        "summary persistence join",
    )
    .await?;

    Ok(MultiRtReport {
        cold,
        summary,
        ready_before_release,
        final_persisted_bytes,
    })
}

fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn main() {
    logl::log(
        level::INFO,
        format_args!(
            "rusqlite-multirt: blueprint start workers={} executor=native-worker+current-thread",
            WORKERS
        ),
    );
    let runtime = match t::runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("rusqlite-multirt: runtime build failed {error}"),
            );
            platform::poll_once();
            return;
        }
    };

    match runtime.block_on(run_multirt()) {
        Ok(report) => logl::log(
            level::INFO,
            format_args!(
                "rusqlite-multirt: PASS sqlite_version={} db_path={} cold_read=native-worker cold_loaded_bytes={} workers={} ready={} distinct_slots={} max_active={} operations={} checksum={} worker_image_bytes={} digest=0x{:016X} final_persisted_bytes={} users={} settings={} sample={}:{}:{}:{}",
                report.cold.sqlite_version,
                DB_PATH,
                report.cold.loaded_bytes,
                report.summary.workers,
                report.ready_before_release,
                report.summary.distinct_slots,
                report.summary.max_active,
                report.summary.operations,
                report.summary.checksum,
                report.summary.worker_image_bytes,
                report.summary.digest,
                report.final_persisted_bytes,
                report.cold.user_count,
                report.cold.settings_count,
                report.cold.sample.user_id,
                report.cold.sample.nickname,
                report.cold.sample.settings_id,
                report.cold.sample.language,
            ),
        ),
        Err(error) => logl::log(
            level::ERROR,
            format_args!("rusqlite-multirt: FAILED {error}"),
        ),
    }
    drop(runtime);
    platform::poll_once();
}
