//! Sequential tile loading as a local task on the Blueprint's Tokio runtime.
use crate::{Tiles, viewport::TileKey};
use anyhow::{Context, Result};
use image::RgbaImage;
use std::collections::VecDeque;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

pub struct Completion {
    pub key: TileKey,
    pub image: Result<RgbaImage>,
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
    mut source: impl TileSource,
    mut commands: watch::Receiver<Vec<TileKey>>,
    results: mpsc::UnboundedSender<Completion>,
) {
    let mut pending = VecDeque::new();
    loop {
        if pending.is_empty() {
            if commands.changed().await.is_err() {
                return;
            }
            pending = commands.borrow_and_update().clone().into();
        } else {
            match commands.has_changed() {
                Ok(true) => pending = commands.borrow_and_update().clone().into(),
                Ok(false) => {}
                Err(_) => return,
            }
        }
        // The watch channel keeps only the latest viewport demand. An
        // in-flight fetch finishes normally; the UI checks whether it matters.
        let Some(key) = pending.pop_front() else {
            continue;
        };
        let image = source.load(key).await;
        if results.send(Completion { key, image }).is_err() {
            return;
        }
        // Even a burst of immediate cache hits gives the UI task a turn.
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::oneshot;
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
            };
            let mut view = crate::viewport::Viewport::new(map, 256, 280);
            map.x += 128.0;
            view.navigate(map, 256, 280);
            loader.request(view.missing()).unwrap();
            map.zoom(-1);
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
