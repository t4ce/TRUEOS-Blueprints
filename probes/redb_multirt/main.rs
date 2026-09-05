use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use trueos::{logl, logl::level, platform, t};
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition},
};
#[path = "../redb_probe/store.rs"]
mod store;

const DB_PATH: &str = "/common/usersettings-multirt.redb";
const WORKERS: usize = 2;
const BATCHES_PER_WORKER: usize = 16;
const ROWS_PER_BATCH: usize = 16;
const OPS_PER_WORKER: usize = BATCHES_PER_WORKER * ROWS_PER_BATCH;
const START_WAIT_MS: usize = 2_000;
const EVENTS: TableDefinition<u64, u64> = TableDefinition::new("events");
const SUMMARY: TableDefinition<&str, u64> = TableDefinition::new("native_runtime_summary");

type ProbeError = String;

struct ColdReport {
    loaded_bytes: usize,
    persisted_bytes: usize,
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

fn run_cold_read() -> Result<ColdReport, ProbeError> {
    let (database, loaded_bytes) = store::open(DB_PATH)?;
    let image = store::persist(DB_PATH, database)?;
    Ok(ColdReport {
        loaded_bytes,
        persisted_bytes: image.len(),
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
    let database = ImageDatabase::open(&image)?;
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
    .map_err(|_| "worker release timeout".to_string())?;
    if release.load(Ordering::Acquire) == 2 {
        return Err("fleet cancelled".into());
    }
    let now_active = active.fetch_add(1, Ordering::AcqRel) + 1;
    max_active.fetch_max(now_active, Ordering::AcqRel);
    let result = run_worker_transactions(worker, wls_slot, database).await;
    active.fetch_sub(1, Ordering::AcqRel);
    result
}

async fn run_worker_transactions(
    worker: usize,
    wls_slot: usize,
    database: ImageDatabase,
) -> Result<WorkerReport, ProbeError> {
    for batch in 0..BATCHES_PER_WORKER {
        let write = database.database().begin_write().map_err(store::error)?;
        {
            let mut events = write.open_table(EVENTS).map_err(store::error)?;
            for offset in 0..ROWS_PER_BATCH {
                let seq = (batch * ROWS_PER_BATCH + offset) as u64;
                events
                    .insert(seq, (worker as u64 + 1) * 1_000_000 + seq)
                    .map_err(store::error)?;
            }
            if events.len().map_err(store::error)? != ((batch + 1) * ROWS_PER_BATCH) as u64 {
                return Err(format!("worker={worker} batch={batch} row count mismatch"));
            }
        }
        write.commit().map_err(store::error)?;
        t::task::yield_now().await;
        t::time::sleep(t::time::Duration::from_millis(1)).await;
        if t::worker::local_slot() as usize != wls_slot {
            return Err("worker WLS moved".into());
        }
    }

    // Close and reopen each lane's complete redb image before verifying rows.
    let image = database.into_image()?;
    let restored = ImageDatabase::open(&image)?;
    store::verify(&restored)?;
    let read = restored.database().begin_read().map_err(store::error)?;
    let events = read.open_table(EVENTS).map_err(store::error)?;
    if events.len().map_err(store::error)? != OPS_PER_WORKER as u64 {
        return Err("restored row count mismatch".into());
    }
    let mut checksum = 0i64;
    for seq in 0..OPS_PER_WORKER as u64 {
        let expected = (worker as u64 + 1) * 1_000_000 + seq;
        let value = events
            .get(seq)
            .map_err(store::error)?
            .ok_or("restored row missing")?
            .value();
        if value != expected {
            return Err(format!("worker={worker} seq={seq} restored value mismatch"));
        }
        checksum += value as i64;
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

fn summary_values(summary: Summary) -> [(&'static str, u64); 7] {
    [
        ("workers", summary.workers as u64),
        ("operations", summary.operations as u64),
        ("checksum", summary.checksum as u64),
        ("distinct_slots", summary.distinct_slots as u64),
        ("max_active", summary.max_active as u64),
        ("worker_image_bytes", summary.worker_image_bytes as u64),
        ("digest", summary.digest),
    ]
}

fn persist_summary(image: Arc<Vec<u8>>, summary: Summary) -> Result<usize, ProbeError> {
    let database = ImageDatabase::open(&image)?;
    let write = database.database().begin_write().map_err(store::error)?;
    {
        let mut table = write.open_table(SUMMARY).map_err(store::error)?;
        for (key, value) in summary_values(summary) {
            table.insert(key, value).map_err(store::error)?;
        }
    }
    write.commit().map_err(store::error)?;
    let image = store::persist(DB_PATH, database)?;
    let restored = ImageDatabase::open(&image)?;
    let read = restored.database().begin_read().map_err(store::error)?;
    let table = read.open_table(SUMMARY).map_err(store::error)?;
    for (key, expected) in summary_values(summary) {
        if table
            .get(key)
            .map_err(store::error)?
            .is_none_or(|value| value.value() != expected)
        {
            return Err(format!("persisted summary mismatch: {key}"));
        }
    }
    Ok(image.len())
}

async fn join_native<T>(
    mut job: t::worker::JoinHandle<Result<T, ProbeError>>,
    stage: &'static str,
) -> Result<T, ProbeError> {
    match t::time::timeout(t::time::Duration::from_secs(10), &mut job).await {
        Ok(result) => result.map_err(|_| stage.to_string())?,
        Err(_) => {
            logl::log(
                level::ERROR,
                format_args!("redb-multirt: FAIL stage={stage} timeout=true action=draining"),
            );
            let _ = job.await;
            Err(stage.to_string())
        }
    }
}

async fn run_multirt() -> Result<MultiRtReport, ProbeError> {
    logl::log(
        level::INFO,
        format_args!("redb-multirt: phase=cold-read executor=native-worker begin"),
    );
    let cold = join_native(
        t::worker::spawn(run_cold_read).map_err(|_| "cold-read submit".to_string())?,
        "cold-read join",
    )
    .await?;
    logl::log(
        level::INFO,
        format_args!(
            "redb-multirt: phase=cold-read status=ok loaded_bytes={} persisted_bytes={}",
            cold.loaded_bytes, cold.persisted_bytes
        ),
    );
    // Each native lane owns a separate redb database and image. Persistence
    // remains explicit async file I/O after closing the database.
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
    .map_err(|_| "insufficient native capacity".to_string())?;

    logl::log(
        level::INFO,
        format_args!(
            "redb-multirt: phase=pressure executor=native-worker workers={} batches_per_worker={} rows_per_batch={} begin",
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
                .map_err(|_| "worker runtime".to_string())?;
            let result = runtime.block_on(run_worker(
                worker, image, ready, release, active, max_active,
            ));
            drop(runtime);
            result
        }) {
            Ok(job) => workers.push(job),
            Err(_) => {
                failure = Some("worker submit".to_string());
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
        failure.get_or_insert("worker ready timeout".to_string());
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
        return Err(format!(
            "native fleet did not become concurrent ready={} joined={} max_active={max_active} distinct_slots={distinct_slots} expected={} wait_ms={waited_ms}",
            ready_before_release,
            reports.len(),
            WORKERS,
        ));
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
        return Err(format!(
            "operation total={operations} expected={}",
            WORKERS * OPS_PER_WORKER
        ));
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
            .map_err(|_| "summary persistence submit".to_string())?,
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
            "redb-multirt: blueprint start workers={} executor=native-worker+current-thread",
            WORKERS
        ),
    );
    let runtime = match t::runtime::current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("redb-multirt: runtime build failed {error}"),
            );
            platform::poll_once();
            return;
        }
    };

    match runtime.block_on(run_multirt()) {
        Ok(report) => logl::log(
            level::INFO,
            format_args!(
                "redb-multirt: PASS backend=redb-4.2.0 db_path={} cold_read=native-worker cold_loaded_bytes={} workers={} ready={} distinct_slots={} max_active={} operations={} checksum={} worker_image_bytes={} digest=0x{:016X} final_persisted_bytes={} users=1 settings=1 reopened=true",
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
            ),
        ),
        Err(error) => logl::log(level::ERROR, format_args!("redb-multirt: FAILED {error}")),
    }
    drop(runtime);
    platform::poll_once();
}
