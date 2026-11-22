mod camera;
#[cfg(any(target_os = "trueos", target_os = "zkvm", test))]
mod logo;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod menu;
// trueos-blueprint: features=["tokio-net-probe", "ui4-scene"]
#[cfg(any(target_os = "trueos", target_os = "zkvm", test))]
mod loader;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod presenter;
mod ram_cache;
mod viewport;

use anyhow::{Context, Result, ensure};
use camera::Map;
use image::RgbaImage;
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
const FOOTER: u32 = 24;
const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

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
                    // A corrupt or incorrectly sized cache entry must not
                    // survive failed downloads and poison every later retry.
                    std::fs::remove_file(path).context("discard invalid cached tile PNG")?;
                }
            }
        }
        let bytes = self
            .client
            .get(format!("{}/{z}/{x}/{y}.png", self.base))
            .send()
            .await
            .context("fetch tile request")?
            .error_for_status()
            .context("fetch tile HTTP status")?
            .bytes()
            .await
            .context("fetch tile body")?;
        ensure!(bytes.len() <= 2 * 1024 * 1024, "tile too large");
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .context("decode tile PNG")?
            .to_rgba8();
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
    trueos::logl::log_record(
        trueos::logl::level::IMPORTANT,
        "osm",
        format_args!("startup runtime=tokio-current-thread phase=build"),
    );
    let runtime = trueos::runtime::current_thread_net().build()?;
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(run_ui()))
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn run_ui() -> Result<()> {
    use tokio::time::{Instant, sleep_until};
    use trueos::ui4_scene::{
        Error as UiError, Frame, POINTER_BUTTON_MIDDLE, POINTER_BUTTON_PRIMARY,
    };

    const FRAME_INTERVAL: Duration = Duration::from_millis(16);
    const RETRY_INTERVAL: Duration = Duration::from_millis(2);
    const DRAG_BUTTONS: u32 = POINTER_BUTTON_PRIMARY | POINTER_BUTTON_MIDDLE;

    let tiles = Tiles::new()?;
    let mut loader = loader::Loader::start(tiles);
    let mut map = Map::new(51.471336, 13.827807, 17);
    let mut frame = Frame::open_immutable(80, 80, WIDTH, HEIGHT + FOOTER)
        .map_err(|e| anyhow::anyhow!("open map: {e:?}"))?;
    frame
        .set_primary_activation(true)
        .map_err(|e| anyhow::anyhow!("map pointer setup: {e:?}"))?;
    let icon = logo::decode()?;
    frame
        .enable_collapse(icon.width(), icon.height(), icon.as_raw())
        .map_err(|e| anyhow::anyhow!("map collapse setup: {e:?}"))?;
    let mut menu =
        menu::MapMenu::new(&mut frame).map_err(|e| anyhow::anyhow!("map menu setup: {e:?}"))?;
    let window = frame.window_id();
    trueos::logl::log_record(
        trueos::logl::level::IMPORTANT,
        "osm",
        format_args!(
            "execution=tokio-local runtime=current-thread loader=cooperative window={window} extent={}x{}",
            frame.width(),
            frame.height()
        ),
    );
    let mut view = viewport::Viewport::new(map, frame.width(), frame.height());
    let mut presenter = presenter::Presenter::default();
    let mut displayed = (map, frame.width(), frame.height());
    let mut submitting = displayed;
    let mut revision = 1u64;
    let mut published = 0u64;
    let mut pending_resize = None;
    let mut drag = None;
    let mut presentations = 0u64;
    let mut discarded = 0u64;
    loader.request(view.missing())?;

    println!("OSM: drag to move, wheel to zoom; https://www.openstreetmap.org/copyright");
    loop {
        let frame_deadline = Instant::now() + FRAME_INTERVAL;
        trueos::vsys::poll_once();
        let (displayed_map, width, height) = displayed;
        let map_height = height.saturating_sub(FOOTER);
        menu.tick(&mut frame, |x, y| {
            viewport::coordinates_at(displayed_map, width, height, x, y)
        })
        .map_err(|e| anyhow::anyhow!("map menu: {e:?}"))?;

        let mut navigated = false;
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|e| anyhow::anyhow!("map resize event: {e:?}"))?
        {
            pending_resize = Some((event.width, event.height));
        }
        // UI4 also emits a middle-button pan event for each pointer sample.
        // Handle the gestures and wheel in pointer queue order.
        while frame
            .take_pan_event()
            .map_err(|e| anyhow::anyhow!("map pan event: {e:?}"))?
            .is_some()
        {}
        while let Some(event) = frame
            .take_pointer_event()
            .map_err(|e| anyhow::anyhow!("map pointer event: {e:?}"))?
        {
            let over_map = (0..width as i32).contains(&event.local_x)
                && (0..map_height as i32).contains(&event.local_y);
            if let Some((source, x, y)) = drag
                && source == event.source
            {
                let held = event.buttons_down & DRAG_BUTTONS != 0;
                if held || event.buttons_released & DRAG_BUTTONS != 0 {
                    navigated |= map.pan(event.x as i32 - x, event.y as i32 - y);
                }
                drag = held.then_some((source, event.x as i32, event.y as i32));
            }
            if drag.is_none() && over_map && event.buttons_pressed & DRAG_BUTTONS != 0 {
                drag = Some((event.source, event.x as i32, event.y as i32));
            }
            if over_map && event.wheel != 0 {
                navigated |= map.zoom_at(
                    i32::from(event.wheel),
                    f64::from(event.local_x) - f64::from(width / 2),
                    f64::from(event.local_y) - f64::from(map_height / 2),
                );
            }
        }

        if !presenter.is_publishing()
            && let Some((width, height)) = pending_resize
        {
            let (old_width, old_height) = (frame.width(), frame.height());
            if (width, height) == (old_width, old_height) {
                pending_resize = None;
            } else {
                match frame.resize(width, height) {
                    Ok(()) => {
                        pending_resize = None;
                        navigated = true;
                        trueos::logl::log_record(
                            trueos::logl::level::IMPORTANT,
                            "osm",
                            format_args!(
                                "resize staged window={window} previous_extent={old_width}x{old_height} extent={width}x{height} revision={}",
                                revision + 1
                            ),
                        );
                    }
                    Err(UiError::Busy) => {}
                    Err(error) => {
                        trueos::logl::log_record(
                            trueos::logl::level::ERROR,
                            "osm",
                            format_args!(
                                "resize failed window={window} previous_extent={old_width}x{old_height} requested_extent={width}x{height} revision={revision} error={error:?}"
                            ),
                        );
                        return Err(anyhow::anyhow!("map resize {width}x{height}: {error:?}"));
                    }
                }
            }
        }
        if navigated {
            view.navigate(map, frame.width(), frame.height());
        }
        let completed = apply_completions(&mut loader, &mut view, &mut discarded)?;
        if navigated {
            loader.request(view.missing())?;
        }
        if navigated || completed {
            revision += 1;
        }

        let mut retry = pending_resize.is_some();
        if published != revision {
            if !presenter.is_publishing() {
                submitting = (map, frame.width(), frame.height());
            }
            match presenter.step(&mut frame, revision, || view.pixels()) {
                Ok(presenter::Status::Published(presented)) => {
                    published = presented;
                    let (map, width, height) = submitting;
                    let resized = (displayed.1, displayed.2) != (width, height);
                    displayed = submitting;
                    retry |= published != revision;
                    presentations += 1;
                    if resized || presentations <= 4 || presentations % 120 == 0 {
                        trueos::logl::log_record(
                            trueos::logl::level::IMPORTANT,
                            "osm",
                            format_args!(
                                "immediate viewport window={window} frame={presentations} revision={published} zoom={:.2} tiles={} extent={width}x{height} resized={resized} stale-skipped={discarded}",
                                map.display_zoom(),
                                map.zoom,
                            ),
                        );
                    }
                }
                Ok(presenter::Status::Busy) => retry = true,
                Ok(presenter::Status::Idle) => {}
                Err(error) => return Err(anyhow::anyhow!("present map sprite: {error:?}")),
            }
        }
        let now = Instant::now();
        if frame_deadline <= now {
            tokio::task::yield_now().await;
        } else {
            let wake = if retry {
                frame_deadline.min(now + RETRY_INTERVAL)
            } else {
                frame_deadline
            };
            sleep_until(wake).await;
        }
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
                Err(error) => {
                    trueos::logl::log(
                        trueos::logl::level::ERROR,
                        format_args!(
                            "osm: tile {}/{}/{} failed retry_ms={} error={error:#}",
                            completion.key.z,
                            completion.key.x,
                            completion.key.y,
                            completion.retry_after.unwrap_or_default().as_millis(),
                        ),
                    );
                }
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
    fn corrupt_disk_and_http_error_bodies_are_not_retained_as_tiles() {
        use std::io::{Read, Write};
        let cache = std::env::temp_dir().join(format!(
            "trueos-osm-corrupt-cache-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&cache).unwrap();
        let path = cache.join("2-1-1.png");
        std::fs::write(&path, b"old corrupt PNG").unwrap();
        let pixels = RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (status, body) in [
                ("503 Service Unavailable", b"<html>busy</html>".as_slice()),
                ("200 OK", b"<html>not PNG</html>".as_slice()),
                ("200 OK", png.as_slice()),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let len = stream.read(&mut buffer).unwrap();
                    assert_ne!(len, 0);
                    request.extend_from_slice(&buffer[..len]);
                }
                assert!(request.starts_with(b"GET /2/1/1.png HTTP/1.1\r\n"));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let mut tiles = Tiles {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            cache: Some(cache.clone()),
            ram: ram_cache::RamCache::new().unwrap(),
            base,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime.block_on(tiles.tile(2, 1, 1)).unwrap_err();
        assert!(format!("{error:#}").contains("fetch tile HTTP status"));
        assert!(!path.exists());
        let error = runtime.block_on(tiles.tile(2, 1, 1)).unwrap_err();
        assert!(format!("{error:#}").contains("decode tile PNG"));
        assert!(!path.exists());
        assert_eq!(runtime.block_on(tiles.tile(2, 1, 1)).unwrap(), pixels);
        assert_eq!(image::open(&path).unwrap().to_rgba8(), pixels);
        server.join().unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

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
