//! Compose the current view from decoded tiles and recently visible zoom levels.
use crate::{FOOTER, Map};
use font8x8::UnicodeFonts;
use image::{Rgba, RgbaImage};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
};

const RECENT_TILES: usize = 64;
const TILE_PITCH: usize = 256 * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TileKey {
    pub z: u8,
    pub x: i64,
    pub y: i64,
}
impl TileKey {
    pub fn cache_key(self) -> String {
        format!("{}-{}-{}", self.z, self.x, self.y)
    }
}

#[derive(Clone, Copy)]
struct Placement {
    key: TileKey,
    x: f64,
    y: f64,
    scale: f64,
}

impl Placement {
    fn bounds(self) -> Bounds {
        let edge = 256.0 * self.scale;
        Bounds {
            left: first_pixel(self.x),
            top: first_pixel(self.y),
            right: first_pixel(self.x + edge),
            bottom: first_pixel(self.y + edge),
        }
    }
}

// Rasterize tile edges at pixel centers so neighbouring tiles meet exactly.
fn first_pixel(edge: f64) -> i64 {
    (edge - 0.5).ceil() as i64
}

#[derive(Clone, Copy)]
struct Bounds {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Bounds {
    fn intersect(self, other: Self) -> Option<Self> {
        let bounds = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (bounds.left < bounds.right && bounds.top < bounds.bottom).then_some(bounds)
    }
}

struct CachedTile {
    pixels: RgbaImage,
    last_visible: u64,
}

pub struct Viewport {
    map: Map,
    pixels: RgbaImage,
    placements: Vec<Placement>,
    wanted: HashSet<TileKey>,
    tiles: HashMap<TileKey, CachedTile>,
    navigation: u64,
    dirty: bool,
    columns: Vec<usize>,
}

impl Viewport {
    pub fn new(map: Map, width: u32, height: u32) -> Self {
        let placements = placements(map, width, height);
        Self {
            map,
            pixels: frame_pixels(width, height),
            wanted: placements.iter().map(|p| p.key).collect(),
            placements,
            tiles: HashMap::new(),
            navigation: 0,
            dirty: false,
            columns: Vec::new(),
        }
    }

    #[cfg(test)]
    fn coordinates_at(&self, x: i32, y: i32) -> Option<(f64, f64)> {
        coordinates_at(self.map, self.pixels.width(), self.pixels.height(), x, y)
    }

    pub fn pixels(&mut self) -> &RgbaImage {
        if self.dirty {
            self.compose();
            self.dirty = false;
        }
        &self.pixels
    }

    pub fn missing(&self) -> Vec<TileKey> {
        let mut seen = HashSet::new();
        self.placements
            .iter()
            .filter_map(|p| {
                (!self.tiles.contains_key(&p.key) && seen.insert(p.key)).then_some(p.key)
            })
            .collect()
    }

    pub fn navigate(&mut self, map: Map, width: u32, height: u32) {
        if self.pixels.dimensions() != (width, height) {
            self.pixels = frame_pixels(width, height);
        }
        self.map = map;
        self.placements = placements(map, width, height);
        self.wanted.clear();
        self.wanted.extend(self.placements.iter().map(|p| p.key));
        self.navigation += 1;

        let mut recent = Vec::new();
        for (key, tile) in &mut self.tiles {
            if self.wanted.contains(key) {
                tile.last_visible = self.navigation;
            } else {
                recent.push((tile.last_visible, *key));
            }
        }
        if recent.len() > RECENT_TILES {
            let remove = recent.len() - RECENT_TILES;
            recent.select_nth_unstable_by_key(remove, |(last_visible, _)| *last_visible);
            for (_, key) in recent.into_iter().take(remove) {
                self.tiles.remove(&key);
            }
        }
        self.dirty = true;
    }

    /// Accept a completion only when the latest viewport still needs its key.
    pub fn complete(&mut self, key: TileKey, tile: RgbaImage) -> bool {
        if tile.dimensions() != (256, 256) || !self.wanted.contains(&key) {
            return false;
        }
        self.tiles.insert(
            key,
            CachedTile {
                pixels: opaque_tile(tile),
                last_visible: self.navigation,
            },
        );
        self.dirty = true;
        true
    }

    fn compose(&mut self) {
        let width = self.pixels.width();
        let height = self.pixels.height().saturating_sub(FOOTER);
        let map_bounds = Bounds {
            left: 0,
            top: 0,
            right: i64::from(width),
            bottom: i64::from(height),
        };
        let pitch = width as usize * 4;
        let map_pixels = &mut self.pixels.as_mut()[..pitch * height as usize];
        map_pixels.fill(255);
        let mut gaps = Vec::new();
        for &p in &self.placements {
            let Some(bounds) = p.bounds().intersect(map_bounds) else {
                continue;
            };
            if let Some(tile) = self.tiles.get(&p.key) {
                paint_tile(
                    map_pixels,
                    pitch,
                    &tile.pixels,
                    p,
                    bounds,
                    &mut self.columns,
                );
            } else {
                gaps.push(bounds);
            }
        }

        if gaps.is_empty() {
            return;
        }
        let mut fallback: Vec<_> = self
            .tiles
            .iter()
            .filter(|(key, _)| key.z != self.map.zoom)
            .collect();
        // Nearer levels take precedence. Prefer more detail at equal distance.
        fallback.sort_unstable_by_key(|(key, _)| (Reverse(key.z.abs_diff(self.map.zoom)), key.z));
        let (left, top) = origin(self.map, width, height);
        let world = self.map.world_size();
        for (&key, tile) in fallback {
            let scale = 2f64.powf(self.map.display_zoom() - f64::from(key.z));
            let edge = 256.0 * scale;
            let x = key.x as f64 * edge - left;
            let y = key.y as f64 * edge - top;
            if y + edge <= 0.0 || y >= f64::from(height) {
                continue;
            }
            let first_wrap = ((-x - edge) / world).floor() as i64 + 1;
            let end_wrap = ((f64::from(width) - x) / world).ceil() as i64;
            for wrap in first_wrap..end_wrap {
                let p = Placement {
                    key,
                    x: x + wrap as f64 * world,
                    y,
                    scale,
                };
                let bounds = p.bounds();
                for &gap in &gaps {
                    if let Some(clip) = bounds.intersect(gap) {
                        paint_tile(map_pixels, pitch, &tile.pixels, p, clip, &mut self.columns);
                    }
                }
            }
        }
    }
}

pub fn coordinates_at(map: Map, width: u32, height: u32, x: i32, y: i32) -> Option<(f64, f64)> {
    let height = height.saturating_sub(FOOTER);
    if !(0..width as i32).contains(&x) || !(0..height as i32).contains(&y) {
        return None;
    }
    let (left, top) = origin(map, width, height);
    let world = map.world_size();
    let px = (left + f64::from(x)).rem_euclid(world);
    let py = top + f64::from(y);
    if !(0.0..=world).contains(&py) {
        return None;
    }
    let lon = px / world * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * py / world))
        .sinh()
        .atan()
        .to_degrees();
    Some((lon, lat))
}

fn origin(map: Map, width: u32, map_height: u32) -> (f64, f64) {
    (
        map.x - f64::from(width / 2),
        map.y - f64::from(map_height / 2),
    )
}

fn placements(map: Map, width: u32, height: u32) -> Vec<Placement> {
    let map_height = height.saturating_sub(FOOTER);
    let mut out = Vec::new();
    if width == 0 || map_height == 0 {
        return out;
    }
    let (left, top) = origin(map, width, map_height);
    let n = 1i64 << map.zoom;
    let scale = map.tile_scale();
    let edge = 256.0 * scale;
    let tile_left = (left / edge).floor() as i64;
    let tile_right = ((left + f64::from(width)) / edge).ceil() as i64;
    let tile_top = ((top / edge).floor() as i64).max(0);
    let tile_bottom = (((top + f64::from(map_height)) / edge).ceil() as i64).min(n);
    for ty in tile_top..tile_bottom {
        for tx in tile_left..tile_right {
            out.push(Placement {
                key: TileKey {
                    z: map.zoom,
                    x: tx.rem_euclid(n),
                    y: ty,
                },
                x: tx as f64 * edge - left,
                y: ty as f64 * edge - top,
                scale,
            });
        }
    }
    // Issue visible requests from the center out.
    let distance = |p: &Placement| {
        let dx = p.x + edge / 2.0 - f64::from(width / 2);
        let dy = p.y + edge / 2.0 - f64::from(map_height / 2);
        dx * dx + dy * dy
    };
    out.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    out
}

fn paint_tile(
    pixels: &mut [u8],
    pitch: usize,
    tile: &RgbaImage,
    p: Placement,
    clip: Bounds,
    columns: &mut Vec<usize>,
) {
    let dest_x = clip.left as usize * 4;
    let row_len = (clip.right - clip.left) as usize * 4;
    let source = tile.as_raw();
    if p.scale == 1.0 {
        let source_x = (clip.left - first_pixel(p.x)) as usize * 4;
        let source_y = first_pixel(p.y);
        for y in clip.top..clip.bottom {
            let src = (y - source_y) as usize * TILE_PITCH + source_x;
            let dst = y as usize * pitch + dest_x;
            pixels[dst..dst + row_len].copy_from_slice(&source[src..src + row_len]);
        }
        return;
    }

    let inverse = p.scale.recip();
    let sample = |pixel: i64, origin: f64| {
        ((pixel as f64 + 0.5 - origin) * inverse)
            .floor()
            .clamp(0.0, 255.0) as usize
    };
    columns.clear();
    columns.extend((clip.left..clip.right).map(|x| sample(x, p.x) * 4));
    let mut last_sample = None;
    for y in clip.top..clip.bottom {
        let sy = sample(y, p.y);
        let dst = y as usize * pitch + dest_x;
        if last_sample == Some(sy) {
            pixels.copy_within(dst - pitch..dst - pitch + row_len, dst);
        } else {
            let row = &source[sy * TILE_PITCH..(sy + 1) * TILE_PITCH];
            for (pixel, &sx) in pixels[dst..dst + row_len]
                .chunks_exact_mut(4)
                .zip(columns.iter())
            {
                pixel.copy_from_slice(&row[sx..sx + 4]);
            }
            last_sample = Some(sy);
        }
    }
}

fn opaque_tile(mut tile: RgbaImage) -> RgbaImage {
    // Blend once against the map background, keeping the viewport suitable for
    // UI4's opaque copy path even when an alternate provider returns alpha.
    for pixel in tile.as_mut().chunks_exact_mut(4) {
        if pixel[3] != 255 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
    tile
}

fn frame_pixels(width: u32, height: u32) -> RgbaImage {
    let mut pixels = RgbaImage::from_pixel(width, height, Rgba([255; 4]));
    let footer_top = height.saturating_sub(FOOTER);
    for (i, ch) in "(c) OpenStreetMap contributors | ODbL".chars().enumerate() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (y, row) in glyph.iter().enumerate() {
                for x in 0..8 {
                    let px = 8 + i as u32 * 8 + x;
                    let py = footer_top + 8 + y as u32;
                    if row & (1 << x) != 0 && px < width && py < height {
                        pixels.put_pixel(px, py, Rgba([20, 20, 20, 255]));
                    }
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map() -> Map {
        Map {
            x: 384.0,
            y: 384.0,
            zoom: 2,
            local_steps: 0,
        }
    }
    fn tile(color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(256, 256, Rgba(color))
    }
    fn loaded() -> Viewport {
        let mut view = Viewport::new(map(), 256, 256 + FOOTER);
        assert!(view.complete(TileKey { z: 2, x: 1, y: 1 }, tile([40, 80, 120, 255])));
        view
    }
    #[test]
    fn three_local_ticks_reuse_tiles_then_fourth_changes_remote_level() {
        for direction in [-1, 1] {
            let mut next = map();
            let mut view = loaded();
            for tick in 1..=3 {
                next.zoom(direction);
                view.navigate(next, 256, 256 + FOOTER);
                assert_eq!(next.zoom, 2);
                assert_eq!(next.local_steps, tick * direction as i8);
                assert!(view.missing().iter().all(|key| key.z == 2));
                assert_eq!(*view.pixels().get_pixel(128, 128), Rgba([40, 80, 120, 255]));
                if direction == 1 {
                    assert!(view.missing().is_empty());
                }
            }
            next.zoom(direction);
            view.navigate(next, 256, 256 + FOOTER);
            assert_eq!(next.zoom as i32, 2 + direction);
            assert_eq!(next.local_steps, 0);
            assert!(view.missing().iter().all(|key| key.z == next.zoom));
        }
    }
    #[test]
    fn local_zoom_preserves_coordinates_reverses_and_scales_late_tiles() {
        let mut next = map();
        let mut view = Viewport::new(next, 256, 256 + FOOTER);
        let center = view.coordinates_at(128, 128).unwrap();
        let key = TileKey { z: 2, x: 1, y: 1 };
        for _ in 0..3 {
            next.zoom(1);
        }
        view.navigate(next, 256, 256 + FOOTER);
        let after = view.coordinates_at(128, 128).unwrap();
        // Integer viewport origins introduce less than one displayed pixel.
        assert!((center.0 - after.0).abs() < 360.0 / next.world_size());
        assert!((center.1 - after.1).abs() < 360.0 / next.world_size());
        let gradient = RgbaImage::from_fn(256, 256, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        assert!(view.complete(key, gradient));
        let pixel = view.pixels().get_pixel(0, 0);
        assert!(pixel[0] > 40 && pixel[0] < 60);
        for _ in 0..3 {
            next.zoom(-1);
        }
        assert_eq!(next.zoom, 2);
        assert_eq!(next.local_steps, 0);
        assert!((next.x - map().x).abs() < 1e-8);
        assert!((next.y - map().y).abs() < 1e-8);
        view.navigate(next, 256, 256 + FOOTER);
        assert_eq!(*view.pixels().get_pixel(0, 0), Rgba([0, 0, 0, 255]));
        assert!(view.missing().is_empty());
    }
    #[test]
    fn local_zoom_clamps_at_provider_limits() {
        let mut next = Map::new(0.0, 0.0, 0);
        next.zoom(-1);
        assert_eq!(next.display_zoom(), 0.0);
        for _ in 0..100 {
            next.zoom(1);
        }
        assert_eq!(next.display_zoom(), 19.0);
        assert_eq!(next.local_steps, 0);
        for _ in 0..100 {
            next.zoom(-1);
        }
        assert_eq!(next.display_zoom(), 0.0);
    }
    #[test]
    fn clicked_coordinates_use_view_origin_footer_and_world_wrap() {
        let view = Viewport::new(Map::new(0.0, 0.0, 2), 256, 256 + FOOTER);
        let (lon, lat) = view.coordinates_at(128, 128).unwrap();
        assert!(lon.abs() < 1e-8 && lat.abs() < 1e-8);
        assert!(view.coordinates_at(128, 256).is_none());
        assert!(view.coordinates_at(-1, 128).is_none());
        let view = Viewport::new(Map::new(0.0, 180.0, 2), 256, 256 + FOOTER);
        assert_eq!(view.coordinates_at(128, 128).unwrap().0, -180.0);
        let (_, north) = view.coordinates_at(128, 0).unwrap();
        assert!(north > 0.0);
    }
    #[test]
    fn pan_repositions_ready_pixels_and_exposes_white_before_any_completion() {
        let mut view = loaded();
        let mut next = map();
        next.x += 128.0;
        view.navigate(next, 256, 256 + FOOTER);
        assert_eq!(*view.pixels().get_pixel(127, 0), Rgba([40, 80, 120, 255]));
        assert_eq!(*view.pixels().get_pixel(128, 0), Rgba([255; 4]));
        assert_eq!(view.missing(), [TileKey { z: 2, x: 2, y: 1 }]);
        assert!(view.complete(view.missing()[0], tile([1, 2, 3, 255])));
        assert_eq!(*view.pixels().get_pixel(128, 0), Rgba([1, 2, 3, 255]));
        assert!(view.pixels().pixels().all(|p| p[3] == 255));
    }
    #[test]
    fn pan_with_no_overlap_stays_white_in_both_directions() {
        for dx in [-600.0, 600.0] {
            let mut view = loaded();
            let mut next = map();
            next.x += dx;
            view.navigate(next, 256, 256 + FOOTER);
            assert!(
                view.pixels()
                    .enumerate_pixels()
                    .filter(|(_, y, _)| *y < 256)
                    .all(|(_, _, p)| *p == Rgba([255; 4]))
            );
        }
    }
    #[test]
    fn zoom_out_scales_existing_view_and_discards_old_zoom_completion() {
        let mut view = loaded();
        let mut next = map();
        for _ in 0..4 {
            next.zoom(-1);
        }
        view.navigate(next, 256, 256 + FOOTER);
        assert_eq!(*view.pixels().get_pixel(64, 64), Rgba([40, 80, 120, 255]));
        assert_eq!(*view.pixels().get_pixel(191, 191), Rgba([40, 80, 120, 255]));
        for (x, y) in [(63, 64), (192, 191), (0, 0)] {
            assert_eq!(*view.pixels().get_pixel(x, y), Rgba([255; 4]));
        }
        let before = view.pixels().clone();
        assert!(!view.complete(TileKey { z: 2, x: 1, y: 1 }, tile([255, 0, 0, 255])));
        assert_eq!(*view.pixels(), before);
        assert!(view.complete(TileKey { z: 1, x: 0, y: 0 }, tile([5, 6, 7, 255])));
        assert_eq!(*view.pixels().get_pixel(0, 0), Rgba([5, 6, 7, 255]));
    }
    #[test]
    fn zoom_in_samples_existing_pixels_in_the_new_coordinate_system() {
        let mut view = Viewport::new(map(), 256, 256 + FOOTER);
        let gradient = RgbaImage::from_fn(256, 256, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        view.complete(TileKey { z: 2, x: 1, y: 1 }, gradient);
        let mut next = map();
        for _ in 0..4 {
            next.zoom(1);
        }
        view.navigate(next, 256, 256 + FOOTER);
        assert_eq!(*view.pixels().get_pixel(0, 0), Rgba([64, 64, 0, 255]));
        assert_eq!(*view.pixels().get_pixel(255, 255), Rgba([191, 191, 0, 255]));
    }
    #[test]
    fn completion_uses_latest_pan_resize_position_and_never_overwrites_footer() {
        let mut view = Viewport::new(map(), 256, 256 + FOOTER);
        let key = view.missing()[0];
        let mut next = map();
        next.x += 128.0;
        view.navigate(next, 512, 512 + FOOTER);
        assert!(view.complete(key, tile([12, 34, 56, 255])));
        assert_eq!(*view.pixels().get_pixel(0, 128), Rgba([12, 34, 56, 255]));
        assert_eq!(*view.pixels().get_pixel(256, 128), Rgba([255; 4]));
        assert_eq!(*view.pixels().get_pixel(0, 512), Rgba([255; 4]));
        next.x += 512.0;
        view.navigate(next, 512, 512 + FOOTER);
        let before = view.pixels().clone();
        assert!(!view.complete(key, tile([255, 0, 0, 255])));
        assert_eq!(*view.pixels(), before);
    }
    #[test]
    fn recent_tiles_make_pan_and_zoom_round_trips_immediate() {
        let mut view = loaded();
        let original = view.pixels().clone();
        let mut camera = map();

        camera.pan(-300, 0);
        view.navigate(camera, 256, 256 + FOOTER);
        assert_ne!(*view.pixels(), original);
        camera.pan(300, 0);
        view.navigate(camera, 256, 256 + FOOTER);
        assert!(view.missing().is_empty());
        assert_eq!(*view.pixels(), original);

        camera.zoom_at(4, -32.0, 32.0);
        view.navigate(camera, 256, 256 + FOOTER);
        assert!(!view.missing().is_empty());
        assert_eq!(*view.pixels().get_pixel(0, 255), Rgba([40, 80, 120, 255]));
        camera.zoom_at(-4, -32.0, 32.0);
        view.navigate(camera, 256, 256 + FOOTER);
        assert!(view.missing().is_empty());
        assert_eq!(*view.pixels(), original);
    }

    #[test]
    fn wrapped_pan_keeps_tiles_aligned_across_the_antimeridian() {
        let mut camera = Map { x: 8.0, ..map() };
        let mut view = Viewport::new(camera, 32, 32 + FOOTER);
        let west = Rgba([20, 40, 60, 255]);
        let east = Rgba([60, 40, 20, 255]);
        assert!(view.complete(TileKey { z: 2, x: 3, y: 1 }, tile(west.0)));
        assert!(view.complete(TileKey { z: 2, x: 0, y: 1 }, tile(east.0)));
        assert_eq!(*view.pixels().get_pixel(7, 0), west);
        assert_eq!(*view.pixels().get_pixel(8, 0), east);
        camera.pan(16, 0);
        view.navigate(camera, 32, 32 + FOOTER);
        assert!(view.missing().is_empty());
        assert_eq!(*view.pixels().get_pixel(23, 0), west);
        assert_eq!(*view.pixels().get_pixel(24, 0), east);
    }

    #[test]
    fn exact_tiles_replace_fallback_and_keep_the_opaque_frame() {
        let mut view = loaded();
        let mut camera = map();
        camera.zoom(4);
        view.navigate(camera, 256, 256 + FOOTER);
        let key = TileKey { z: 3, x: 2, y: 2 };
        assert!(view.complete(key, tile([1, 5, 9, 128])));
        assert_eq!(*view.pixels().get_pixel(0, 0), Rgba([128, 130, 132, 255]));
        assert_eq!(*view.pixels().get_pixel(128, 0), Rgba([40, 80, 120, 255]));
        assert!(view.pixels().pixels().all(|p| p[3] == 255));
    }

    #[test]
    fn resize_preserves_center_and_reuses_full_visible_tile() {
        let mut view = loaded();
        view.navigate(map(), 512, 512 + FOOTER);
        assert_eq!(*view.pixels().get_pixel(128, 128), Rgba([40, 80, 120, 255]));
        assert_eq!(*view.pixels().get_pixel(383, 383), Rgba([40, 80, 120, 255]));
        assert_eq!(*view.pixels().get_pixel(127, 128), Rgba([255; 4]));
        view.navigate(map(), 12, 16);
        assert!(view.missing().is_empty());
        assert!(view.pixels().pixels().all(|p| p[3] == 255));
    }
    #[test]
    fn one_wrapped_tile_completion_paints_all_current_placements() {
        let mut view = Viewport::new(Map::new(0.0, 0.0, 0), 512, 256 + FOOTER);
        assert_eq!(view.missing(), [TileKey { z: 0, x: 0, y: 0 }]);
        assert!(view.complete(view.missing()[0], tile([12, 34, 56, 255])));
        assert!(
            view.pixels()
                .enumerate_pixels()
                .filter(|(_, y, _)| *y < 256)
                .all(|(_, _, p)| *p == Rgba([12, 34, 56, 255]))
        );
        assert!(view.missing().is_empty());
    }
}
