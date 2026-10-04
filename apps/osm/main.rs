// trueos-blueprint: features=["tokio-net-probe", "ui4-scene"]
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod presenter;
mod ram_cache;

use anyhow::{Context, Result, ensure};
use font8x8::UnicodeFonts;
use image::{Rgba, RgbaImage};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
const FOOTER: u32 = 24;
const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

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
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
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
        let key = format!("{z}-{x}-{y}");
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
    async fn render(&mut self, map: &Map, width: u32, height: u32) -> RgbaImage {
        let map_height = height.saturating_sub(FOOTER);
        let footer_top = map_height;
        let mut out = RgbaImage::from_pixel(width, height, Rgba([230, 230, 230, 255]));
        let left = map.x.floor() as i64 - width as i64 / 2;
        let top = map.y.floor() as i64 - map_height as i64 / 2;
        let n = 1i64 << map.zoom;
        if width != 0 && map_height != 0 {
            for ty in top.div_euclid(256)..=(top + map_height as i64 - 1).div_euclid(256) {
                if !(0..n).contains(&ty) {
                    continue;
                }
                for tx in left.div_euclid(256)..=(left + width as i64 - 1).div_euclid(256) {
                    match self.tile(map.zoom, tx.rem_euclid(n), ty).await {
                        Ok(tile) => image::imageops::overlay(
                            &mut out,
                            &tile,
                            tx * 256 - left,
                            ty * 256 - top,
                        ),
                        Err(error) => eprintln!("tile {}/{tx}/{ty}: {error:#}", map.zoom),
                    }
                }
            }
        }
        for y in footer_top..height {
            for x in 0..width {
                out.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let label = "(c) OpenStreetMap contributors | ODbL";
        for (i, ch) in label.chars().enumerate() {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                for (y, row) in glyph.iter().enumerate() {
                    for x in 0..8 {
                        if row & (1 << x) != 0 {
                            let px = 8 + i as u32 * 8 + x;
                            let py = footer_top + 8 + y as u32;
                            if px < width && py < height {
                                out.put_pixel(px, py, Rgba([20, 20, 20, 255]));
                            }
                        }
                    }
                }
            }
        }
        out
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
    use trueos::ui4_scene::{Frame, PanPhase};
    let rt = trueos::runtime::current_thread_net().build()?;
    let mut tiles = Tiles::new()?;
    let mut map = Map::new(51.471336, 13.827807, 17);
    let mut frame = Frame::open_immutable(80, 80, WIDTH, HEIGHT + FOOTER)
        .map_err(|e| anyhow::anyhow!("open map: {e:?}"))?;
    println!("OSM: pan gesture to move, wheel to zoom; https://www.openstreetmap.org/copyright");
    let mut dirty = true;
    loop {
        // Resize is a staged app-owned repaint, not a broker-scaled snapshot.
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|e| anyhow::anyhow!("map resize event: {e:?}"))?
        {
            if (event.width, event.height) != (frame.width(), frame.height()) {
                frame.resize(event.width, event.height).map_err(|e| {
                    anyhow::anyhow!("map resize {}x{}: {e:?}", event.width, event.height)
                })?;
                trueos::logl::log(
                    trueos::logl::level::IMPORTANT,
                    format_args!(
                        "osm: resize staged {}x{} -> {}x{}",
                        event.old_width, event.old_height, event.width, event.height
                    ),
                );
                dirty = true;
            }
        }
        if dirty {
            let pixels = rt.block_on(tiles.render(&map, frame.width(), frame.height()));
            if let Err(error) = presenter::present(&mut frame, &pixels) {
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
        }
        while let Ok(Some(event)) = frame.take_pan_event() {
            if matches!(event.phase, PanPhase::Begin | PanPhase::Update) {
                map.x -= event.dx as f64;
                map.y = (map.y - event.dy as f64).clamp(0.0, (1u32 << map.zoom) as f64 * 256.0);
                dirty = true;
            }
        }
        while let Ok(Some(event)) = frame.take_pointer_event() {
            if event.wheel != 0 {
                map.zoom(event.wheel as i32);
                dirty = true;
            }
        }
        trueos::vsys::poll_once();
        trueos::vsys::sleep_ms(16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
