#[derive(Clone, Debug)]
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<bool>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, seed_demo: bool) -> Self {
        let mut canvas = Self {
            width,
            height,
            pixels: vec![false; width.saturating_mul(height)],
        };
        if seed_demo {
            canvas.seed_demo();
        }
        canvas
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        let Ok(x) = usize::try_from(x) else {
            return false;
        };
        let Ok(y) = usize::try_from(y) else {
            return false;
        };
        if x >= self.width || y >= self.height {
            return false;
        }
        self.pixels[y * self.width + x]
    }

    pub fn set(&mut self, x: i32, y: i32, value: bool) -> bool {
        let Ok(x) = usize::try_from(x) else {
            return false;
        };
        let Ok(y) = usize::try_from(y) else {
            return false;
        };
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = y * self.width + x;
        let changed = self.pixels[index] != value;
        self.pixels[index] = value;
        changed
    }

    pub fn toggle(&mut self, x: i32, y: i32) -> bool {
        let Ok(x) = usize::try_from(x) else {
            return false;
        };
        let Ok(y) = usize::try_from(y) else {
            return false;
        };
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = y * self.width + x;
        self.pixels[index] = !self.pixels[index];
        true
    }

    pub fn clear(&mut self) {
        self.pixels.fill(false);
    }

    pub fn invert(&mut self) {
        for pixel in &mut self.pixels {
            *pixel = !*pixel;
        }
    }

    pub fn snapshot(&self) -> Vec<bool> {
        self.pixels.clone()
    }

    pub fn restore(&mut self, pixels: Vec<bool>) -> bool {
        if pixels.len() != self.pixels.len() {
            return false;
        }
        self.pixels = pixels;
        true
    }

    pub fn count_lit(&self) -> usize {
        self.pixels.iter().filter(|pixel| **pixel).count()
    }

    pub fn seed_demo(&mut self) {
        self.clear();
        if self.width < 24 || self.height < 20 {
            return;
        }

        self.rectangle(3, 3, self.width as i32 - 4, self.height as i32 - 4);

        let center_x = self.width as i32 / 3;
        self.line(center_x - 12, 10, center_x + 12, 10);
        self.line(center_x, 10, center_x, self.height as i32 - 11);
        self.line(center_x - 8, self.height as i32 - 12, center_x + 8, self.height as i32 - 12);

        let planet_x = self.width as i32 * 3 / 4;
        let planet_y = self.height as i32 / 2;
        for (dx, dy) in [
            (0, -9),
            (-5, -8),
            (5, -8),
            (-8, -5),
            (8, -5),
            (-9, 0),
            (9, 0),
            (-8, 5),
            (8, 5),
            (-5, 8),
            (5, 8),
            (0, 9),
        ] {
            self.set(planet_x + dx, planet_y + dy, true);
        }
        self.line(planet_x - 13, planet_y + 3, planet_x + 13, planet_y - 3);
        self.line(planet_x - 10, planet_y + 6, planet_x + 10, planet_y + 1);

        for (x, y) in [
            (10, 8),
            (16, self.height as i32 - 10),
            (self.width as i32 - 16, 9),
            (self.width as i32 - 11, self.height as i32 - 11),
        ] {
            self.star(x, y);
        }
    }

    fn rectangle(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
        self.line(left, top, right, top);
        self.line(right, top, right, bottom);
        self.line(right, bottom, left, bottom);
        self.line(left, bottom, left, top);
    }

    fn star(&mut self, x: i32, y: i32) {
        for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            self.set(x + dx, y + dy, true);
        }
    }

    fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.set(x0, y0, true);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let doubled = 2 * error;
            if doubled >= dy {
                error += dy;
                x0 += sx;
            }
            if doubled <= dx {
                error += dx;
                y0 += sy;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Canvas;

    #[test]
    fn set_toggle_and_bounds_are_predictable() {
        let mut canvas = Canvas::new(4, 3, false);
        assert!(canvas.set(1, 1, true));
        assert!(canvas.get(1, 1));
        assert!(!canvas.set(1, 1, true));
        assert!(canvas.toggle(1, 1));
        assert!(!canvas.get(1, 1));
        assert!(!canvas.set(-1, 0, true));
        assert!(!canvas.set(4, 0, true));
    }

    #[test]
    fn snapshots_restore_only_matching_canvas_sizes() {
        let mut canvas = Canvas::new(4, 3, false);
        canvas.set(2, 2, true);
        let snapshot = canvas.snapshot();
        canvas.clear();
        assert!(canvas.restore(snapshot));
        assert!(canvas.get(2, 2));
        assert!(!canvas.restore(vec![false; 2]));
    }

    #[test]
    fn demo_seed_draws_something_on_a_large_canvas() {
        let canvas = Canvas::new(96, 64, true);
        assert!(canvas.count_lit() > 0);
    }
}
