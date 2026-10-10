#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod compact;
#[cfg(any(target_os = "trueos", target_os = "zkvm", test))]
mod logo;
// trueos-blueprint: features=["tokio-net-probe", "ui4-scene"]
#[cfg(any(target_os = "trueos", target_os = "zkvm", test))]
mod loader;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod presenter;
mod ram_cache;
mod viewport;

use anyhow::{Context, Result, ensure};
use image::RgbaImage;
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
const FOOTER: u32 = 24;
const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone, Copy)]
struct Map {
    x: f64,
    y: f64,
    zoom: u8,
}
impl Map {
    fn new(lat: f64, lon: f64, zoom: u8) -> Self {
        let n = (1u32 << zoom) as f64;
        let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
        Self {
            x: (lon + 180.0) / 360.0 * n * 256.0,
            y: (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n * 256.0,
            zoom,
        }
    }
    fn zoom(&mut self, delta: i32) {
        let next = (self.zoom as i32 + delta.signum()).clamp(0, 19) as u8;
        let scale = 2f64.powi(next as i32 - self.zoom as i32);
        self.x *= scale;
        self.y *= scale;
        self.zoom = next;
    }
}

struct Tiles {
    client: reqwest::Client,
    cache: Option<PathBuf>,
    ram: ram_cache::RamCache,
    base: String,
}
impl Tiles {
    fn new() -> Result<Self> {
        let base = std::env::var("OSM_TILE_URL")
            .unwrap_or_else(|_| "https://tile.openstreetmap.org".into());
        ensure!(base.starts_with("https://"), "OSM_TILE_URL must use HTTPS");
        let cache = PathBuf::from(
            std::env::var("OSM_CACHE_DIR").unwrap_or_else(|_| "osm-tile-cache".into()),
        );
        let ram_only = std::env::var("OSM_CACHE_MODE").is_ok_and(|mode| mode == "ram");
        ensure!(
            !ram_only || base.trim_end_matches('/') != "https://tile.openstreetmap.org",
            "public OSM tiles require persistent caching; use a provider permitting RAM-only caching"
        );
        let cache = if ram_only {
            None
        } else {
            std::fs::create_dir_all(&cache).context("create persistent tile cache")?;
            Some(cache)
        };
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent("TRUEOS-OSM/0.1")
                .timeout(Duration::from_secs(30))
                .build()?,
            cache,
            ram: ram_cache::RamCache::new()?,
            base: base.trim_end_matches('/').into(),
        })
    }
    async fn tile(&mut self, z: u8, x: i64, y: i64) -> Result<RgbaImage> {
        let key = viewport::TileKey { z, x, y }.cache_key();
        if let Some(tile) = self.ram.get(&key)? {
            return Ok(tile);
        }
        let path = self
            .cache
            .as_ref()
            .map(|cache| cache.join(format!("{key}.png")));
        if let Some(path) = &path {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Some(age) = meta
                    .modified()
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .filter(|age| *age < TTL)
                {
                    if let Ok(image) = image::open(path) {
                        let image = image.to_rgba8();
                        if image.dimensions() == (256, 256) {
                            self.ram.insert(key, &image, TTL - age)?;
                            return Ok(image);
                        }
                    }
                }
            }
        }
        let bytes = self
            .client
            .get(format!("{}/{z}/{x}/{y}.png", self.base))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        ensure!(bytes.len() <= 2 * 1024 * 1024, "tile too large");
        let image =
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.to_rgba8();
        ensure!(
            image.dimensions() == (256, 256),
            "unexpected tile dimensions"
        );
        if let Some(path) = path {
            std::fs::write(path, &bytes).context("cache tile")?;
        }
        self.ram.insert(key, &image, TTL)?;
        Ok(image)
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    async fn render(&mut self, map: &Map, width: u32, height: u32) -> RgbaImage {
        let mut view = viewport::Viewport::new(*map, width, height);
        for key in view.missing() {
            match self.tile(key.z, key.x, key.y).await {
                Ok(tile) => {
                    view.complete(key, tile);
                }
                Err(error) => eprintln!("tile {}/{}/{}: {error:#}", key.z, key.x, key.y),
            }
        }
        view.pixels().clone()
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn main() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut tiles = Tiles::new()?;
    let map = Map::new(51.471336, 13.827807, 17);
    rt.block_on(tiles.render(&map, WIDTH, HEIGHT + FOOTER))
        .save("osm-demo.png")?;
    println!("Saved osm-demo.png; attribution: https://www.openstreetmap.org/copyright");
    Ok(())
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn main() -> Result<()> {
    trueos::logl::log(
        trueos::logl::level::IMPORTANT,
        format_args!("osm: startup runtime=tokio-current-thread phase=build"),
    );
    let runtime = trueos::runtime::current_thread_net().build()?;
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(run_ui()))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn run_ui() -> Result<()> {
    use trueos::ui4_scene::{Error as UiError, Frame, PanPhase};
    let tiles = Tiles::new()?;
    let mut loader = loader::Loader::start(tiles);
    trueos::logl::log(
        trueos::logl::level::IMPORTANT,
        format_args!("osm: execution=tokio-local runtime=current-thread loader=cooperative"),
    );
    let mut map = Map::new(51.471336, 13.827807, 17);
    let mut frame = Frame::open_immutable(80, 80, WIDTH, HEIGHT + FOOTER)
        .map_err(|e| anyhow::anyhow!("open map: {e:?}"))?;
    let mut compact = compact::Compact::new(&mut frame)
        .map_err(|e| anyhow::anyhow!("map compact/menu setup: {e:?}"))?;
    let mut view = viewport::Viewport::new(map, frame.width(), frame.height());
    println!("OSM: pan gesture to move, wheel to zoom; https://www.openstreetmap.org/copyright");
    let mut dirty = true;
    let mut request = true;
    let mut pending_resize = None;
    let mut presentations = 0u64;
    let mut discarded = 0u64;
    loop {
        let was_collapsed = compact.collapsed();
        if compact
            .tick(&mut frame, &view)
            .await
            .map_err(|e| anyhow::anyhow!("map collapse/restore: {e:?}"))?
        {
            if !was_collapsed {
                loader.request(Vec::new())?;
                pending_resize = None;
            }
            // Keep the expanded view intact; an already-running fetch can
            // still finish, but the compact frame displays only its logo.
            dirty |= apply_completions(&mut loader, &mut view, &mut discarded)?;
            trueos::vsys::poll_once();
            tokio::time::sleep(Duration::from_millis(16)).await;
            continue;
        }
        if was_collapsed {
            dirty = true;
            request = true;
        }
        let mut navigated = false;
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|e| anyhow::anyhow!("map resize event: {e:?}"))?
        {
            pending_resize = Some((event.width, event.height));
        }
        if let Some((width, height)) = pending_resize {
            if (width, height) == (frame.width(), frame.height()) {
                pending_resize = None;
            } else {
                match frame.resize(width, height) {
                    Ok(()) => {
                        pending_resize = None;
                        navigated = true;
                        trueos::logl::log(
                            trueos::logl::level::IMPORTANT,
                            format_args!("osm: resize staged {}x{}", width, height),
                        );
                    }
                    Err(UiError::Busy) => {}
                    Err(e) => return Err(anyhow::anyhow!("map resize {width}x{height}: {e:?}")),
                }
            }
        }
        // Drain input before considering any completion from an older view.
        while let Some(event) = frame
            .take_pan_event()
            .map_err(|e| anyhow::anyhow!("map pan event: {e:?}"))?
        {
            if matches!(event.phase, PanPhase::Begin | PanPhase::Update) {
                map.x -= event.dx as f64;
                map.y = (map.y - event.dy as f64).clamp(0.0, (1u32 << map.zoom) as f64 * 256.0);
                navigated = true;
            }
        }
        while let Some(event) = frame
            .take_pointer_event()
            .map_err(|e| anyhow::anyhow!("map pointer event: {e:?}"))?
        {
            if event.wheel != 0 {
                map.zoom(event.wheel as i32);
                navigated = true;
            }
        }
        if navigated {
            view.navigate(map, frame.width(), frame.height());
            dirty = true;
            request = true;
        }
        dirty |= apply_completions(&mut loader, &mut view, &mut discarded)?;
        if dirty {
            if let Err(error) = presenter::present(&mut frame, view.pixels()).await {
                trueos::logl::log(
                    trueos::logl::level::ERROR,
                    format_args!(
                        "osm: presentation failed error={error:?} center=({:.0},{:.0}) zoom={} extent={}x{}",
                        map.x,
                        map.y,
                        map.zoom,
                        frame.width(),
                        frame.height()
                    ),
                );
                return Err(anyhow::anyhow!("present map sprite: {error:?}"));
            }
            dirty = false;
            presentations += 1;
            if presentations <= 4 || presentations % 120 == 0 {
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "osm: immediate viewport frame={} zoom={} extent={}x{} missing={} stale-skipped={}",
                        presentations,
                        map.zoom,
                        frame.width(),
                        frame.height(),
                        view.missing().len(),
                        discarded
                    ),
                );
            }
        }
        if request {
            loader.request(view.missing())?;
            request = false;
        }
        trueos::vsys::poll_once();
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn apply_completions(
    loader: &mut loader::Loader,
    view: &mut viewport::Viewport,
    discarded: &mut u64,
) -> Result<bool> {
    let mut dirty = false;
    loop {
        match loader.completions.try_recv() {
            Ok(completion) => match completion.image {
                Ok(tile) => {
                    if view.complete(completion.key, tile) {
                        dirty = true;
                    } else {
                        *discarded += 1;
                    }
                }
                Err(error) => eprintln!(
                    "tile {}/{}/{}: {error:#}",
                    completion.key.z, completion.key.x, completion.key.y
                ),
            },
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(dirty),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(anyhow::anyhow!("tile loader stopped"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn warmed_render_reuses_decoded_ram_tiles_after_disk_removal() {
        let path =
            std::env::temp_dir().join(format!("trueos-osm-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let map = Map::new(51.471336, 13.827807, 17);
        let left = map.x.floor() as i64 - WIDTH as i64 / 2;
        let top = map.y.floor() as i64 - HEIGHT as i64 / 2;
        let tile = RgbaImage::from_pixel(256, 256, Rgba([100, 150, 200, 255]));
        for ty in top.div_euclid(256)..=(top + HEIGHT as i64 - 1).div_euclid(256) {
            for tx in left.div_euclid(256)..=(left + WIDTH as i64 - 1).div_euclid(256) {
                tile.save(path.join(format!("17-{tx}-{ty}.png"))).unwrap();
            }
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut tiles = Tiles {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(100))
                .build()
                .unwrap(),
            base: "https://127.0.0.1:1".into(),
            cache: Some(path.clone()),
            ram: ram_cache::RamCache::new().unwrap(),
        };
        let started = std::time::Instant::now();
        let cold = rt.block_on(tiles.render(&map, WIDTH, HEIGHT + FOOTER));
        let cold_time = started.elapsed();
        assert_eq!(*cold.get_pixel(0, 0), Rgba([100, 150, 200, 255]));
        std::fs::remove_dir_all(path).unwrap();
        let started = std::time::Instant::now();
        let warm = rt.block_on(tiles.render(&map, WIDTH, HEIGHT + FOOTER));
        let warm_time = started.elapsed();
        assert_eq!(cold, warm);
        eprintln!("fixture render: disk+decode+RAM insert {cold_time:?}, RAM reuse {warm_time:?}");
    }
    #[test]
    fn tiny_resize_keeps_valid_opaque_frame_and_clips_footer() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut tiles = Tiles {
            client: reqwest::Client::new(),
            base: "https://127.0.0.1:1".into(),
            cache: None,
            ram: ram_cache::RamCache::new().unwrap(),
        };
        let map = Map::new(0.0, 0.0, 0);
        let image = rt.block_on(tiles.render(&map, 12, 16));
        assert_eq!(image.dimensions(), (12, 16));
        assert!(image.pixels().all(|pixel| pixel[3] == 255));
    }
    #[test]
    fn mercator_coordinates() {
        let center = Map::new(0.0, 0.0, 0);
        assert_eq!((center.x, center.y), (128.0, 128.0));
        let location = Map::new(51.471336, 13.827807, 17);
        assert_eq!(
            (location.x as i64 / 256, location.y as i64 / 256),
            (70570, 43605)
        );
        assert!(Map::new(90.0, 0.0, 17).y.is_finite());
    }
}
