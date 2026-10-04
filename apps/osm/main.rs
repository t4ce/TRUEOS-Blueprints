// trueos-blueprint: features=["tokio-net-probe", "ui4-scene"]
use anyhow::{Context, Result, ensure};
use font8x8::UnicodeFonts;
use image::{Rgba, RgbaImage};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
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
    cache: PathBuf,
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
        std::fs::create_dir_all(&cache).context("create persistent tile cache")?;
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent("TRUEOS-OSM/0.1")
                .timeout(Duration::from_secs(30))
                .build()?,
            cache,
            base: base.trim_end_matches('/').into(),
        })
    }
    async fn tile(&self, z: u8, x: i64, y: i64) -> Result<RgbaImage> {
        let path = self.cache.join(format!("{z}-{x}-{y}.png"));
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta
                .modified()
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < TTL)
            {
                if let Ok(image) = image::open(&path) {
                    return Ok(image.to_rgba8());
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
        std::fs::write(path, &bytes).context("cache tile")?;
        Ok(image)
    }
    async fn render(&self, map: &Map) -> RgbaImage {
        let mut out = RgbaImage::from_pixel(WIDTH, HEIGHT + 24, Rgba([230, 230, 230, 255]));
        let left = map.x.floor() as i64 - WIDTH as i64 / 2;
        let top = map.y.floor() as i64 - HEIGHT as i64 / 2;
        let n = 1i64 << map.zoom;
        for ty in top.div_euclid(256)..=(top + HEIGHT as i64 - 1).div_euclid(256) {
            if !(0..n).contains(&ty) {
                continue;
            }
            for tx in left.div_euclid(256)..=(left + WIDTH as i64 - 1).div_euclid(256) {
                match self.tile(map.zoom, tx.rem_euclid(n), ty).await {
                    Ok(tile) => {
                        image::imageops::overlay(&mut out, &tile, tx * 256 - left, ty * 256 - top)
                    }
                    Err(error) => eprintln!("tile {}/{tx}/{ty}: {error:#}", map.zoom),
                }
            }
        }
        for y in HEIGHT..HEIGHT + 24 {
            for x in 0..WIDTH {
                out.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let label = "(c) OpenStreetMap contributors | ODbL";
        for (i, ch) in label.chars().enumerate() {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                for (y, row) in glyph.iter().enumerate() {
                    for x in 0..8 {
                        if row & (1 << x) != 0 {
                            out.put_pixel(
                                8 + i as u32 * 8 + x,
                                HEIGHT + 8 + y as u32,
                                Rgba([20, 20, 20, 255]),
                            );
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
    let tiles = Tiles::new()?;
    let map = Map::new(51.471336, 13.827807, 17);
    rt.block_on(tiles.render(&map)).save("osm-demo.png")?;
    println!("Saved osm-demo.png; attribution: https://www.openstreetmap.org/copyright");
    Ok(())
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn main() -> Result<()> {
    use trueos::ui4_scene::{Damage, Error, Frame, PanPhase};
    let rt = trueos::runtime::current_thread_net().build()?;
    let tiles = Tiles::new()?;
    let mut map = Map::new(51.471336, 13.827807, 17);
    let mut frame = Frame::open_immutable(80, 80, WIDTH, HEIGHT + 24)
        .map_err(|e| anyhow::anyhow!("open map: {e:?}"))?;
    println!("OSM: pan gesture to move, wheel to zoom; https://www.openstreetmap.org/copyright");
    let mut dirty = true;
    loop {
        if dirty {
            let pixels = rt.block_on(tiles.render(&map));
            loop {
                match frame.begin(trueos::ui4_scene::rgba(230, 230, 230, 255)) {
                    Ok(()) => break,
                    Err(Error::Busy) => trueos::vsys::sleep_ms(8),
                    Err(e) => anyhow::bail!("begin map: {e:?}"),
                }
            }
            loop {
                match frame.write_opaque_rgba8(pixels.as_raw()) {
                    Ok(()) => break,
                    Err(Error::Busy) => trueos::vsys::sleep_ms(8),
                    Err(e) => anyhow::bail!("paint map: {e:?}"),
                }
            }
            frame
                .publish(Damage::full(WIDTH, HEIGHT + 24))
                .map_err(|e| anyhow::anyhow!("publish map: {e:?}"))?;
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
