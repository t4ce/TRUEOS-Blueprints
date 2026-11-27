use lico_graph::{DIRECTORY, DanceClueSnapshot, LEASE_REFRESH_NS, Lease, MAX_SNAPSHOT_BYTES};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};
use trueos::{async_fs, clock::monotonic_nanos, logl};

const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn serve(mut current: watch::Receiver<Option<Arc<DanceClueSnapshot>>>) {
    let instance = format!("lico-{:x}-{:x}", monotonic_nanos(), trueos::rng::u64());
    let (graph_path, lease_path) = lico_graph::paths(&instance).expect("valid Lico instance");
    let refresh = Duration::from_nanos(LEASE_REFRESH_NS);
    let mut timer = interval_at(Instant::now() + refresh, refresh);
    timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut published = None;
    let mut leased = None;
    let mut has_files = false;
    let mut last_error = None;

    loop {
        let closed = tokio::select! {
            change = current.changed() => change.is_err(),
            _ = timer.tick() => false,
        };
        let snapshot = if closed {
            None
        } else {
            current.borrow_and_update().clone()
        };
        let result = match snapshot {
            Some(snapshot) => {
                has_files = true;
                let result = if published == Some(snapshot.run) {
                    Ok(())
                } else {
                    let result = publish_graph(&graph_path, &snapshot).await;
                    if result.is_ok() {
                        published = Some(snapshot.run);
                    }
                    result
                };
                if result.is_ok() {
                    // A replacement or Stop that arrived during the graph write takes precedence.
                    if !matches!(current.has_changed(), Ok(false)) {
                        continue;
                    }
                    let result = write_lease(&lease_path, snapshot.run).await;
                    if result.is_ok() && leased != Some(snapshot.run) {
                        leased = Some(snapshot.run);
                        logl::log_record(
                            logl::level::INFO,
                            "lico",
                            format_args!(
                                "DanceClueSnapshot run={} started_ns={} nodes={} patterns={} arrangements={}",
                                snapshot.run,
                                snapshot.started_ns,
                                snapshot.graph.nodes.len(),
                                snapshot.graph.patterns.len(),
                                snapshot.graph.arrangements.len(),
                            ),
                        );
                    }
                    result
                } else {
                    result
                }
            }
            None if has_files => {
                let result = retire(&graph_path, &lease_path).await;
                if result.is_ok() {
                    has_files = false;
                    published = None;
                    if let Some(run) = leased.take() {
                        logl::log_record(
                            logl::level::INFO,
                            "lico",
                            format_args!("DanceClueSnapshot stopped run={run}"),
                        );
                    }
                }
                result
            }
            None => Ok(()),
        };
        let error = result.err();
        if let Some(error) = error.as_ref().filter(|_| error != last_error) {
            logl::log_record(
                logl::level::WARN,
                "lico",
                format_args!("DanceClueSnapshot: {error}"),
            );
        }
        last_error = error;
        if closed {
            break;
        }
    }
}

async fn publish_graph(path: &str, snapshot: &DanceClueSnapshot) -> Result<(), String> {
    snapshot.validate()?;
    let bytes = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err("DSP graph exceeds snapshot size limit".into());
    }
    fs_operation(async_fs::create_dir_all(DIRECTORY.as_bytes())).await?;
    fs_operation(async_fs::write_file(path.as_bytes(), &bytes)).await
}

async fn write_lease(path: &str, run: u64) -> Result<(), String> {
    let lease = Lease {
        run,
        refreshed_ns: monotonic_nanos(),
    };
    let bytes = serde_json::to_vec(&lease).map_err(|error| error.to_string())?;
    fs_operation(async_fs::write_file(path.as_bytes(), &bytes)).await
}

async fn retire(graph: &str, lease: &str) -> Result<(), String> {
    // Once the lease disappears, readers stop even if removing the graph requires a retry.
    for path in [lease, graph] {
        fs_operation(async {
            match async_fs::remove(path.as_bytes()).await {
                Err(async_fs::ERR_NOT_FOUND) => Ok(()),
                result => result,
            }
        })
        .await?;
    }
    Ok(())
}

async fn fs_operation(operation: impl Future<Output = Result<(), i32>>) -> Result<(), String> {
    timeout(IO_TIMEOUT, operation)
        .await
        .map_err(|_| "shared filesystem operation timed out".to_owned())?
        .map_err(|code| format!("shared filesystem error ({code})"))
}
