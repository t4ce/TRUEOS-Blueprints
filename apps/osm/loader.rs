//! Sequential tile loading as a local task on the Blueprint's Tokio runtime.
use crate::{Tiles, viewport::TileKey};
use anyhow::{Context, Result};
use image::RgbaImage;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::Instant,
};

pub struct Completion {
    pub key: TileKey,
    pub image: Result<RgbaImage>,
    pub retry_after: Option<Duration>,
}
pub struct Loader {
    requests: watch::Sender<Vec<TileKey>>,
    pub completions: mpsc::UnboundedReceiver<Completion>,
    task: JoinHandle<()>,
}
impl Loader {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub fn start(tiles: Tiles) -> Self {
        Self::with_source(tiles)
    }
    fn with_source(source: impl TileSource + 'static) -> Self {
        let (requests, commands) = watch::channel(Vec::new());
        let (results, completions) = mpsc::unbounded_channel();
        let task = tokio::task::spawn_local(serve(source, commands, results));
        Self {
            requests,
            completions,
            task,
        }
    }
    pub fn request(&self, missing: Vec<TileKey>) -> Result<()> {
        self.requests.send(missing).context("tile loader stopped")
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.task.abort();
    }
}

trait TileSource {
    async fn load(&mut self, key: TileKey) -> Result<RgbaImage>;
}
impl TileSource for Tiles {
    async fn load(&mut self, key: TileKey) -> Result<RgbaImage> {
        self.tile(key.z, key.x, key.y).await
    }
}

async fn serve(
    source: impl TileSource,
    commands: watch::Receiver<Vec<TileKey>>,
    results: mpsc::UnboundedSender<Completion>,
) {
    serve_with_retry(
        source,
        commands,
        results,
        RetryPolicy {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(30),
        },
    )
    .await;
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    initial: Duration,
    maximum: Duration,
}
impl RetryPolicy {
    fn delay(self, failures: u32) -> Duration {
        self.initial
            .checked_mul(
                1u32.checked_shl(failures.saturating_sub(1))
                    .unwrap_or(u32::MAX),
            )
            .unwrap_or(self.maximum)
            .min(self.maximum)
    }
}
struct Retry {
    failures: u32,
    deadline: Instant,
}

fn replace_demand(
    commands: &mut watch::Receiver<Vec<TileKey>>,
    pending: &mut VecDeque<TileKey>,
    retries: &mut HashMap<TileKey, Retry>,
) {
    let demand = commands.borrow_and_update();
    let wanted: HashSet<_> = demand.iter().copied().collect();
    retries.retain(|key, _| wanted.contains(key));
    let now = Instant::now();
    *pending = demand
        .iter()
        .copied()
        .filter(|key| retries.get(key).is_none_or(|retry| retry.deadline <= now))
        .collect();
}

async fn serve_with_retry(
    mut source: impl TileSource,
    mut commands: watch::Receiver<Vec<TileKey>>,
    results: mpsc::UnboundedSender<Completion>,
    policy: RetryPolicy,
) {
    let mut pending = VecDeque::new();
    let mut retries = HashMap::<TileKey, Retry>::new();
    loop {
        match commands.has_changed() {
            Ok(true) => replace_demand(&mut commands, &mut pending, &mut retries),
            Ok(false) => {}
            Err(_) => return,
        }
        if pending.is_empty() {
            if let Some(deadline) = retries.values().map(|retry| retry.deadline).min() {
                match tokio::time::timeout_at(deadline, commands.changed()).await {
                    Ok(Ok(())) => {
                        replace_demand(&mut commands, &mut pending, &mut retries);
                    }
                    Ok(Err(_)) => return,
                    Err(_) => {
                        let now = Instant::now();
                        pending.extend(
                            retries
                                .iter()
                                .filter_map(|(key, retry)| (retry.deadline <= now).then_some(*key)),
                        );
                    }
                }
            } else {
                if commands.changed().await.is_err() {
                    return;
                }
                replace_demand(&mut commands, &mut pending, &mut retries);
            }
            continue;
        }
        // The watch channel keeps only the latest viewport demand. An
        // in-flight fetch finishes normally; the UI checks whether it matters.
        let Some(key) = pending.pop_front() else {
            continue;
        };
        let image = source.load(key).await;
        let retry_after = if image.is_err() {
            let retry = retries.entry(key).or_insert(Retry {
                failures: 0,
                deadline: Instant::now(),
            });
            retry.failures = retry.failures.saturating_add(1);
            let delay = policy.delay(retry.failures);
            retry.deadline = Instant::now() + delay;
            Some(delay)
        } else {
            retries.remove(&key);
            None
        };
        if results
            .send(Completion {
                key,
                image,
                retry_after,
            })
            .is_err()
        {
            return;
        }
        // Even a burst of immediate cache hits gives the UI task a turn.
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;
    struct FlakySource {
        first: TileKey,
        failed: bool,
        started: mpsc::UnboundedSender<TileKey>,
    }
    impl TileSource for FlakySource {
        async fn load(&mut self, key: TileKey) -> Result<RgbaImage> {
            self.started.send(key).unwrap();
            if key == self.first && !self.failed {
                self.failed = true;
                anyhow::bail!("decode tile PNG: temporarily unavailable fixture");
            }
            Ok(RgbaImage::from_pixel(
                256,
                256,
                image::Rgba([10, 20, 30, 255]),
            ))
        }
    }

    #[test]
    fn failed_tile_recovers_without_navigation_and_old_demand_cancels_retry() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            for navigate in [false, true] {
                let first = TileKey { z: 2, x: 1, y: 1 };
                let latest = TileKey { z: 1, x: 0, y: 0 };
                let (started_tx, mut started) = mpsc::unbounded_channel();
                let (requests, commands) = watch::channel(Vec::new());
                let (results, mut completions) = mpsc::unbounded_channel();
                let task = tokio::task::spawn_local(serve_with_retry(
                    FlakySource {
                        first,
                        failed: false,
                        started: started_tx,
                    },
                    commands,
                    results,
                    RetryPolicy {
                        initial: Duration::from_millis(10),
                        maximum: Duration::from_millis(40),
                    },
                ));
                requests.send(vec![first]).unwrap();
                let failed = tokio::time::timeout(Duration::from_secs(2), completions.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(failed.image.is_err());
                assert_eq!(failed.retry_after, Some(Duration::from_millis(10)));
                assert_eq!(started.recv().await.unwrap(), first);
                if navigate {
                    requests.send(vec![latest]).unwrap();
                }
                let recovered = tokio::time::timeout(Duration::from_secs(2), completions.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(recovered.key, if navigate { latest } else { first });
                assert!(recovered.image.is_ok());
                assert_eq!(recovered.retry_after, None);
                assert_eq!(started.recv().await.unwrap(), recovered.key);
                // Successful loads stop retrying, and obsolete failures must
                // not wake up later after a newer viewport replaces demand.
                assert!(
                    tokio::time::timeout(Duration::from_millis(60), started.recv())
                        .await
                        .is_err()
                );
                drop(requests);
                task.await.unwrap();
            }
        }));
    }

    #[test]
    fn retry_backoff_is_capped_even_after_many_failures() {
        let policy = RetryPolicy {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(30),
        };
        for (failures, seconds) in [(1, 1), (2, 2), (5, 16), (6, 30), (32, 30), (u32::MAX, 30)] {
            assert_eq!(policy.delay(failures), Duration::from_secs(seconds));
        }
    }

    struct HeldSource {
        owner: std::thread::ThreadId,
        first: TileKey,
        started: mpsc::UnboundedSender<TileKey>,
        gate: Option<oneshot::Receiver<()>>,
    }
    impl TileSource for HeldSource {
        async fn load(&mut self, key: TileKey) -> Result<RgbaImage> {
            assert_eq!(std::thread::current().id(), self.owner);
            self.started.send(key).unwrap();
            if key == self.first {
                self.gate.take().unwrap().await.unwrap();
            }
            Ok(RgbaImage::from_pixel(
                256,
                256,
                image::Rgba([10, 20, 30, 255]),
            ))
        }
    }
    #[test]
    fn pending_fetch_yields_to_navigation_on_the_same_tokio_executor() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let owner = std::thread::current().id();
            let (started_tx, mut started) = mpsc::unbounded_channel();
            let (release, gate) = oneshot::channel();
            let first = TileKey { z: 2, x: 1, y: 1 };
            let obsolete = TileKey { z: 2, x: 2, y: 1 };
            let latest = TileKey { z: 1, x: 0, y: 0 };
            let mut loader = Loader::with_source(HeldSource {
                owner,
                first,
                started: started_tx,
                gate: Some(gate),
            });
            loader.request(vec![first, obsolete]).unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), started.recv())
                    .await
                    .unwrap()
                    .unwrap(),
                first
            );
            // Fetch is pending, while the UI coroutine navigates and paints.
            let mut map = crate::Map {
                x: 384.0,
                y: 384.0,
                zoom: 2,
                local_steps: 0,
            };
            let mut view = crate::viewport::Viewport::new(map, 256, 280);
            map.x += 128.0;
            view.navigate(map, 256, 280);
            loader.request(view.missing()).unwrap();
            for _ in 0..4 {
                map.zoom(-1);
            }
            view.navigate(map, 256, 280);
            loader.request(vec![latest]).unwrap();
            assert_eq!(*view.pixels().get_pixel(0, 0), image::Rgba([255; 4]));
            assert!(matches!(
                loader.completions.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            // Tokio timers must also continue on this executor while fetching.
            tokio::time::sleep(Duration::from_millis(1)).await;
            assert_eq!(std::thread::current().id(), owner);
            release.send(()).unwrap();
            let stale = tokio::time::timeout(Duration::from_secs(2), loader.completions.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stale.key, first);
            assert!(!view.complete(stale.key, stale.image.unwrap()));
            let current = tokio::time::timeout(Duration::from_secs(2), loader.completions.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(current.key, latest);
            assert!(view.complete(current.key, current.image.unwrap()));
            assert_eq!(started.recv().await.unwrap(), latest);
            assert_eq!(std::thread::current().id(), owner);
            drop(loader);
        }));
    }
}
