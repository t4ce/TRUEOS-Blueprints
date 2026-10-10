//! Immediate display state. Only tiles contributing to the current viewport live here.
use crate::{FOOTER, Map};
use font8x8::UnicodeFonts;
use image::{Rgba, RgbaImage};
use std::collections::{HashMap, HashSet};

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
    x: i64,
    y: i64,
}

pub struct Viewport {
    map: Map,
    pixels: RgbaImage,
    placements: Vec<Placement>,
    ready: HashMap<TileKey, RgbaImage>,
}

impl Viewport {
    pub fn new(map: Map, width: u32, height: u32) -> Self {
        let mut pixels = RgbaImage::from_pixel(width, height, Rgba([255; 4]));
        footer(&mut pixels);
        Self {
            map,
            pixels,
            placements: placements(map, width, height),
            ready: HashMap::new(),
        }
    }
    pub fn coordinates_at(&self, x: i32, y: i32) -> Option<(f64, f64)> {
        let height = self.pixels.height().saturating_sub(FOOTER);
        if !(0..self.pixels.width() as i32).contains(&x) || !(0..height as i32).contains(&y) {
            return None;
        }
        let (left, top) = origin(self.map, self.pixels.width(), height);
        let world = (1u32 << self.map.zoom) as f64 * 256.0;
        let px = (left as f64 + x as f64).rem_euclid(world);
        let py = top as f64 + y as f64;
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
    pub fn pixels(&self) -> &RgbaImage {
        &self.pixels
    }
    pub fn missing(&self) -> Vec<TileKey> {
        let mut seen = HashSet::new();
        self.placements
            .iter()
            .filter_map(|p| {
                (!self.ready.contains_key(&p.key) && seen.insert(p.key)).then_some(p.key)
            })
            .collect()
    }
    pub fn navigate(&mut self, map: Map, width: u32, height: u32) {
        let old_height = self.pixels.height().saturating_sub(FOOTER);
        let map_height = height.saturating_sub(FOOTER);
        let (old_left, old_top) = origin(self.map, self.pixels.width(), old_height);
        let (left, top) = origin(map, width, map_height);
        let scale = 2f64.powi(self.map.zoom as i32 - map.zoom as i32);
        let mut pixels = RgbaImage::from_pixel(width, height, Rgba([255; 4]));
        // Reproject displayed map pixels now. Uncovered areas stay opaque white;
        // neither disk/cache lookup, PNG decode nor a network await runs here.
        if scale == 1.0 {
            // Pan and resize are plain clipped row copies.
            let dx = left - old_left;
            let dy = top - old_top;
            let x0 = (-dx).max(0).min(width as i64);
            let x1 = (self.pixels.width() as i64 - dx).min(width as i64).max(x0);
            let y0 = (-dy).max(0).min(map_height as i64);
            let y1 = (old_height as i64 - dy).min(map_height as i64).max(y0);
            let bytes = (x1 - x0) as usize * 4;
            for y in (y0..y1).filter(|_| bytes != 0) {
                let source =
                    ((y + dy) as usize * self.pixels.width() as usize + (x0 + dx) as usize) * 4;
                let dest = (y as usize * width as usize + x0 as usize) * 4;
                pixels.as_mut()[dest..dest + bytes]
                    .copy_from_slice(&self.pixels.as_raw()[source..source + bytes]);
            }
        } else {
            // Precompute the zoom mapping once per column, not once per pixel.
            let columns: Vec<_> = (0..width)
                .map(|x| {
                    let sx =
                        (((left as f64 + x as f64 + 0.5) * scale) - old_left as f64).floor() as i64;
                    (0..self.pixels.width() as i64)
                        .contains(&sx)
                        .then(|| sx as usize * 4)
                })
                .collect();
            for y in 0..map_height {
                let sy = (((top as f64 + y as f64 + 0.5) * scale) - old_top as f64).floor() as i64;
                if !(0..old_height as i64).contains(&sy) {
                    continue;
                }
                let source = sy as usize * self.pixels.width() as usize * 4;
                let dest = y as usize * width as usize * 4;
                for (pixel, column) in pixels.as_mut()[dest..dest + width as usize * 4]
                    .chunks_exact_mut(4)
                    .zip(&columns)
                {
                    if let Some(column) = column {
                        pixel.copy_from_slice(
                            &self.pixels.as_raw()[source + column..source + column + 4],
                        );
                    }
                }
            }
        }
        self.map = map;
        self.placements = placements(map, width, height);
        let needed: HashSet<_> = self.placements.iter().map(|p| p.key).collect();
        self.ready.retain(|key, _| needed.contains(key));
        for placement in &self.placements {
            if let Some(tile) = self.ready.get(&placement.key) {
                image::imageops::overlay(&mut pixels, tile, placement.x, placement.y);
            }
        }
        footer(&mut pixels);
        self.pixels = pixels;
    }
    /// A completion is keyed by tile, not its old screen position or viewport.
    /// Reuse it if still visible (including world wrap); otherwise do no copy.
    pub fn complete(&mut self, key: TileKey, tile: RgbaImage) -> bool {
        if tile.dimensions() != (256, 256) || !self.placements.iter().any(|p| p.key == key) {
            return false;
        }
        for p in self.placements.iter().filter(|p| p.key == key) {
            image::imageops::overlay(&mut self.pixels, &tile, p.x, p.y);
        }
        footer(&mut self.pixels);
        self.ready.insert(key, tile);
        true
    }
}

fn origin(map: Map, width: u32, map_height: u32) -> (i64, i64) {
    (
        map.x.floor() as i64 - width as i64 / 2,
        map.y.floor() as i64 - map_height as i64 / 2,
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
    for ty in top.div_euclid(256)..=(top + map_height as i64 - 1).div_euclid(256) {
        if !(0..n).contains(&ty) {
            continue;
        }
        for tx in left.div_euclid(256)..=(left + width as i64 - 1).div_euclid(256) {
            out.push(Placement {
                key: TileKey {
                    z: map.zoom,
                    x: tx.rem_euclid(n),
                    y: ty,
                },
                x: tx * 256 - left,
                y: ty * 256 - top,
            });
        }
    }
    out
}
fn footer(pixels: &mut RgbaImage) {
    let footer_top = pixels.height().saturating_sub(FOOTER);
    for y in footer_top..pixels.height() {
        for x in 0..pixels.width() {
            pixels.put_pixel(x, y, Rgba([255; 4]));
        }
    }
    for (i, ch) in "(c) OpenStreetMap contributors | ODbL".chars().enumerate() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (y, row) in glyph.iter().enumerate() {
                for x in 0..8 {
                    let px = 8 + i as u32 * 8 + x;
                    let py = footer_top + 8 + y as u32;
                    if row & (1 << x) != 0 && px < pixels.width() && py < pixels.height() {
                        pixels.put_pixel(px, py, Rgba([20, 20, 20, 255]));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map() -> Map {
        Map {
            x: 384.0,
            y: 384.0,
            zoom: 2,
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
        next.zoom(-1);
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
        next.zoom(1);
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
