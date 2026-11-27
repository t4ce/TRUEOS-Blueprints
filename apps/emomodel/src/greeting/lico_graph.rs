use ::lico_graph::{DIRECTORY, Lease, MAX_LEASE_BYTES, MAX_SNAPSHOT_BYTES, paths};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle};
use trueos::{async_fs, clock};

pub use ::lico_graph::DanceClueSnapshot;

const POLL_INTERVAL: Duration = Duration::from_millis(150);

pub struct Observer {
    current: watch::Receiver<Option<Arc<DanceClueSnapshot>>>,
    worker: JoinHandle<()>,
}

impl Observer {
    pub fn new() -> Self {
        let (sender, current) = watch::channel(None);
        let worker = tokio::spawn(observe(sender));
        Self { current, worker }
    }

    pub fn current(&self) -> Option<Arc<DanceClueSnapshot>> {
        self.current.borrow().clone()
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

struct ActiveRun {
    lease: Lease,
    snapshot: Arc<DanceClueSnapshot>,
}

async fn observe(sender: watch::Sender<Option<Arc<DanceClueSnapshot>>>) {
    let mut active = BTreeMap::new();
    loop {
        refresh(&mut active).await;
        let now = clock::monotonic_nanos();
        active.retain(|_, run| run.lease.is_live(now));
        let next = active
            .values()
            .max_by_key(|run| (run.snapshot.started_ns, run.snapshot.run))
            .map(|run| Arc::clone(&run.snapshot));
        sender.send_if_modified(|current| {
            match (&*current, &next) {
                (Some(before), Some(after)) if Arc::ptr_eq(before, after) => return false,
                (None, None) => return false,
                _ => {}
            }
            *current = next;
            true
        });
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn refresh(active: &mut BTreeMap<String, ActiveRun>) {
    let listing = match async_fs::list_dir(DIRECTORY.as_bytes()).await {
        Ok(listing) => listing,
        Err(async_fs::ERR_NOT_FOUND) => {
            active.clear();
            return;
        }
        Err(_) => return,
    };

    let mut previous = std::mem::take(active);
    for entry in listing.entries {
        if entry.kind != async_fs::NodeKind::File {
            continue;
        }
        let Some(instance) = entry.name.strip_suffix(".lease") else {
            continue;
        };
        let Some((snapshot_path, lease_path)) = paths(instance) else {
            continue;
        };
        let prior = previous.remove(instance);
        let lease: Lease = match read_json(&lease_path, MAX_LEASE_BYTES).await {
            Ok(Some(lease)) => lease,
            Ok(None) | Err(async_fs::ERR_NOT_FOUND) => continue,
            Err(_) => {
                if let Some(prior) = prior {
                    active.insert(instance.to_owned(), prior);
                }
                continue;
            }
        };
        if !lease.is_live(clock::monotonic_nanos()) {
            continue;
        }

        let snapshot = match prior {
            Some(prior) if prior.snapshot.run == lease.run => prior.snapshot,
            _ => {
                let Ok(Some(snapshot)) =
                    read_json::<DanceClueSnapshot>(&snapshot_path, MAX_SNAPSHOT_BYTES).await
                else {
                    continue;
                };
                if snapshot.run != lease.run || snapshot.validate().is_err() {
                    continue;
                }
                Arc::new(snapshot)
            }
        };
        if snapshot.started_ns <= lease.refreshed_ns {
            active.insert(instance.to_owned(), ActiveRun { lease, snapshot });
        }
    }
    if listing.truncated {
        active.append(&mut previous);
    }
}

async fn read_json<T: DeserializeOwned>(path: &str, limit: usize) -> Result<Option<T>, i32> {
    let path = path.as_bytes();
    let metadata = async_fs::metadata(path).await?;
    if metadata.kind != async_fs::NodeKind::File || metadata.len > limit as u64 {
        return Ok(None);
    }
    let bytes = async_fs::read_file(path).await?;
    if bytes.len() > limit {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&bytes).ok())
}
