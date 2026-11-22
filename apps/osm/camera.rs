const STEPS_PER_LEVEL: i32 = 4;
const MAX_LEVEL: u8 = 19;

#[derive(Clone, Copy)]
pub(crate) struct Map {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) zoom: u8,
    pub(crate) local_steps: i8,
}

impl Map {
    pub(crate) fn new(lat: f64, lon: f64, zoom: u8) -> Self {
        let world = 256.0 * 2f64.powi(i32::from(zoom));
        let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
        let x = (lon + 180.0) / 360.0 * world;
        let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * world;
        Self {
            x: x.rem_euclid(world),
            y: y.clamp(0.0, world),
            zoom,
            local_steps: 0,
        }
    }

    pub(crate) fn display_zoom(self) -> f64 {
        f64::from(self.zoom) + f64::from(self.local_steps) / f64::from(STEPS_PER_LEVEL)
    }

    pub(crate) fn tile_scale(self) -> f64 {
        2f64.powf(f64::from(self.local_steps) / f64::from(STEPS_PER_LEVEL))
    }

    pub(crate) fn world_size(self) -> f64 {
        256.0 * 2f64.powf(self.display_zoom())
    }

    #[cfg(test)]
    pub(crate) fn zoom(&mut self, delta: i32) -> bool {
        self.zoom_at(delta, 0.0, 0.0)
    }

    /// Each detent changes the display by a quarter level. A batched report
    /// has the same effect as its individual detents at a fixed anchor.
    pub(crate) fn zoom_at(&mut self, delta: i32, offset_x: f64, offset_y: f64) -> bool {
        let level_steps = i32::from(self.zoom) * STEPS_PER_LEVEL;
        let before = level_steps + i32::from(self.local_steps);
        let next = before
            .saturating_add(delta)
            .clamp(0, i32::from(MAX_LEVEL) * STEPS_PER_LEVEL);
        if next == before {
            return false;
        }

        // Change the tile level only after moving a full level away from it.
        // The signed remainder keeps partial steps on either side of it.
        let relative = next - level_steps;
        self.zoom = (i32::from(self.zoom) + relative / STEPS_PER_LEVEL) as u8;
        self.local_steps = (relative % STEPS_PER_LEVEL) as i8;

        let scale = 2f64.powf(f64::from(next - before) / f64::from(STEPS_PER_LEVEL));
        let world = self.world_size();
        self.x = scale_about(self.x, offset_x, scale).rem_euclid(world);
        self.y = scale_about(self.y, offset_y, scale).clamp(0.0, world);
        true
    }

    pub(crate) fn pan(&mut self, dx: i32, dy: i32) -> bool {
        if dx == 0 && dy == 0 {
            return false;
        }
        let world = self.world_size();
        let x = (self.x - f64::from(dx)).rem_euclid(world);
        let y = (self.y - f64::from(dy)).clamp(0.0, world);
        let changed = x != self.x || y != self.y;
        self.x = x;
        self.y = y;
        changed
    }
}

fn scale_about(value: f64, offset: f64, scale: f64) -> f64 {
    let value = value * scale + offset * (scale - 1.0);
    let rounded = value.round();
    // Keep an exact pixel from falling just below an integer boundary after a
    // fractional zoom round trip.
    if (value - rounded).abs() <= value.abs().max(1.0) * f64::EPSILON * 16.0 {
        rounded
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_detents_match_individual_events_across_tile_levels() {
        let mut batch = Map::new(0.0, 0.0, 8);
        let mut individual = batch;
        for delta in [3i32, 10, -2, -17, 8] {
            assert!(batch.zoom_at(delta, 55.0, -87.0));
            for _ in 0..delta.abs() {
                assert!(individual.zoom_at(delta.signum(), 55.0, -87.0));
            }
            assert_eq!(batch.zoom, individual.zoom);
            assert_eq!(batch.local_steps, individual.local_steps);
            assert!((batch.x - individual.x).abs() < 1e-8);
            assert!((batch.y - individual.y).abs() < 1e-8);
        }
    }

    #[test]
    fn off_center_zoom_keeps_the_point_under_the_cursor() {
        let mut map = Map::new(0.0, 0.0, 8);
        let initial = map;
        let (offset_x, offset_y) = (193.0, -119.0);
        let point_at_cursor = |map: Map| {
            (
                (map.x + offset_x) / map.world_size(),
                (map.y + offset_y) / map.world_size(),
            )
        };
        let before = point_at_cursor(map);
        assert!(map.zoom_at(5, offset_x, offset_y));
        let after = point_at_cursor(map);
        assert!((before.0 - after.0).abs() < 1e-12);
        assert!((before.1 - after.1).abs() < 1e-12);
        assert!(map.zoom_at(-5, offset_x, offset_y));
        assert_eq!((map.x, map.y), (initial.x, initial.y));
        assert_eq!(map.display_zoom(), initial.display_zoom());
    }

    #[test]
    fn navigation_wraps_longitude_and_stops_at_latitude_and_zoom_bounds() {
        let mut map = Map::new(0.0, 180.0, 0);
        assert!(map.pan(17, i32::MAX));
        assert_eq!((map.x, map.y), (239.0, 0.0));
        assert!(!map.pan(256, 1));
        assert!(!map.pan(0, 0));
        assert!(!map.zoom_at(i32::MIN, 120.0, -50.0));
        assert!(map.pan(-17, i32::MIN));
        assert_eq!((map.x, map.y), (0.0, 256.0));
        assert!(map.zoom_at(i32::MAX, 0.0, 100.0));
        assert_eq!((map.zoom, map.local_steps), (MAX_LEVEL, 0));
        assert_eq!(map.y, map.world_size());
        assert!(!map.zoom_at(1, -40.0, 80.0));
    }
}
