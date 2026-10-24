//! A viewport over document pixels. Window resizing changes the visible region;
//! it never scales the document's rectangles, baselines, or font sizes.

#[derive(Default, Debug)]
pub(super) struct Viewport {
    width: u32,
    height: u32,
    document_width: u32,
    document_height: u32,
    x: f64,
    y: f64,
}

#[derive(Debug)]
pub(super) struct VisibleText<'a> {
    pub text: &'a str,
    pub x: f32,
    pub baseline_y: f32,
    pub size_px: f32,
    pub advance_px: f32,
}

impl Viewport {
    pub fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn document_extent(&self) -> (u32, u32) {
        (self.document_width, self.document_height)
    }

    pub fn offset(&self) -> (f32, f32) {
        (self.x as f32, self.y as f32)
    }

    pub fn set_document(&mut self, width: u32, height: u32) {
        self.document_width = width;
        self.document_height = height;
        self.x = 0.0;
        self.y = 0.0;
        self.clamp();
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.clamp();
    }

    fn limit(&self) -> (f64, f64) {
        (
            self.document_width.saturating_sub(self.width) as f64,
            self.document_height.saturating_sub(self.height) as f64,
        )
    }

    fn clamp(&mut self) {
        let (max_x, max_y) = self.limit();
        self.x = self.x.clamp(0.0, max_x);
        self.y = self.y.clamp(0.0, max_y);
    }

    pub fn scroll_by(&mut self, dx: f64, dy: f64) -> bool {
        if !dx.is_finite() || !dy.is_finite() {
            return false;
        }
        let previous = (self.x, self.y);
        self.x += dx;
        self.y += dy;
        self.clamp();
        previous != (self.x, self.y)
    }

    /// Navigation keys change the vertical position and retain horizontal scroll.
    pub fn home(&mut self) -> bool {
        self.scroll_by(0.0, -self.y)
    }

    pub fn end(&mut self) -> bool {
        self.scroll_by(0.0, self.limit().1 - self.y)
    }

    pub fn page(&mut self, direction: i32) -> bool {
        self.scroll_by(0.0, self.height as f64 * 0.9 * direction.signum() as f64)
    }

    /// Intersect one rectangle without combining or reordering alpha layers.
    pub fn clip_rect(&self, rect: [f32; 4]) -> Option<[f32; 4]> {
        let [x, y, width, height] = rect;
        if self.width == 0
            || self.height == 0
            || width <= 0.0
            || height <= 0.0
            || !rect.iter().all(|value| value.is_finite())
        {
            return None;
        }
        let left = (x as f64).max(self.x);
        let top = (y as f64).max(self.y);
        let right = (x as f64 + width as f64).min(self.x + self.width as f64);
        let bottom = (y as f64 + height as f64).min(self.y + self.height as f64);
        if right <= left || bottom <= top {
            return None;
        }
        Some([
            (left - self.x) as f32,
            (top - self.y) as f32,
            (right - left) as f32,
            (bottom - top) as f32,
        ])
    }

    /// Borrow only visible Unicode scalar columns, plus one column at each
    /// horizontal edge. The returned whole-run advance retains the original
    /// glyph spacing, including for multibyte UTF-8 text.
    pub fn text_slice<'a>(
        &self,
        text: &'a str,
        x: f32,
        baseline_y: f32,
        size_px: f32,
        advance_px: f32,
    ) -> Option<VisibleText<'a>> {
        if self.width == 0
            || self.height == 0
            || text.is_empty()
            || ![x, baseline_y, size_px, advance_px]
                .iter()
                .all(|value| value.is_finite())
            || size_px <= 0.0
            || size_px > 1_440.0
            || advance_px <= 0.0
        {
            return None;
        }
        // Requested size is conservative because the host snaps raster height
        // down. A pixel guard includes fractional raster bounds at either edge.
        let top = baseline_y as f64 - size_px as f64 - 1.0;
        let bottom = baseline_y as f64 + size_px as f64 / 4.0 + 1.0;
        if bottom <= self.y || top >= self.y + self.height as f64 {
            return None;
        }
        let right = x as f64 + advance_px as f64;
        if right <= self.x - 1.0 || x as f64 >= self.x + self.width as f64 + 1.0 {
            return None;
        }
        let columns = text.chars().count();
        let cell = advance_px as f64 / columns as f64;
        let first =
            (((self.x - x as f64) / cell).floor() - 1.0).clamp(0.0, columns as f64) as usize;
        let last = (((self.x + self.width as f64 - x as f64) / cell).ceil() + 1.0)
            .clamp(0.0, columns as f64) as usize;
        if first >= last {
            return None;
        }
        let first_byte = text
            .char_indices()
            .nth(first)
            .map_or(text.len(), |(byte, _)| byte);
        let last_byte = text
            .char_indices()
            .nth(last)
            .map_or(text.len(), |(byte, _)| byte);
        let text = &text[first_byte..last_byte];
        if text.chars().all(char::is_whitespace) {
            return None;
        }
        let x = (x as f64 + first as f64 * cell - self.x) as f32;
        let baseline_y = (baseline_y as f64 - self.y) as f32;
        let advance_px = ((last - first) as f64 * cell) as f32;
        if ![x, baseline_y, advance_px]
            .iter()
            .all(|value| value.is_finite())
        {
            return None;
        }
        Some(VisibleText {
            text,
            x,
            baseline_y,
            size_px,
            advance_px,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tall_page() -> Viewport {
        let mut viewport = Viewport::default();
        viewport.resize(1280, 720);
        viewport.set_document(1280, 5254);
        viewport
    }

    #[test]
    fn tall_document_preserves_font_size_and_pixel_geometry() {
        let mut v = tall_page();
        assert_eq!(v.extent(), (1280, 720));
        assert_eq!(v.document_extent(), (1280, 5254));
        let text = v.text_slice("visible", 20.0, 100.0, 16.0, 112.0).unwrap();
        assert_eq!(
            (text.x, text.baseline_y, text.size_px, text.advance_px),
            (20., 100., 16., 112.)
        );
        assert!(v.text_slice("below", 20., 1000., 16., 80.).is_none());
        assert!(v.scroll_by(0., 900.));
        let text = v.text_slice("below", 20., 1000., 16., 80.).unwrap();
        assert_eq!(
            (text.x, text.baseline_y, text.size_px, text.advance_px),
            (20., 100., 16., 80.)
        );
        v.resize(640, 500);
        let text = v.text_slice("below", 20., 1000., 16., 80.).unwrap();
        assert_eq!(
            (text.x, text.baseline_y, text.size_px, text.advance_px),
            (20., 100., 16., 80.)
        );
    }

    #[test]
    fn scrolling_clamps_top_middle_bottom_and_resize() {
        let mut v = tall_page();
        assert!(!v.scroll_by(0., -1.));
        assert!(v.page(1));
        assert_eq!(v.offset(), (0., 648.));
        assert!(v.scroll_by(0., 1000.));
        assert_eq!(v.offset(), (0., 1648.));
        assert!(v.end());
        assert_eq!(v.offset(), (0., 4534.));
        assert!(!v.end());
        assert!(!v.scroll_by(0., 100.));
        assert!(v.page(-1));
        assert_eq!(v.offset(), (0., 3886.));
        v.resize(640, 3000);
        assert_eq!(v.offset(), (0., 2254.));
        assert!(v.scroll_by(1000., 0.));
        assert_eq!(v.offset(), (640., 2254.));
        assert!(v.home());
        assert_eq!(v.offset(), (640., 0.));
        assert!(!v.home());
        v.set_document(100, 100);
        assert_eq!(v.offset(), (0., 0.));
        assert!(!v.scroll_by(f64::MAX, f64::MAX));
    }

    #[test]
    fn invalid_scroll_deltas_do_not_change_either_axis() {
        let mut v = tall_page();
        assert!(v.scroll_by(0., 42.));
        for delta in [(f64::NAN, 5.), (5., f64::INFINITY), (0., f64::NEG_INFINITY)] {
            assert!(!v.scroll_by(delta.0, delta.1));
            assert_eq!(v.offset(), (0., 42.));
        }
        assert!(!v.page(0));
    }

    #[test]
    fn rectangles_clip_to_the_viewport_with_fractional_translation() {
        let mut v = Viewport::default();
        v.resize(100, 60);
        v.set_document(300, 300);
        assert!(v.scroll_by(20.25, 40.5));
        assert_eq!(
            v.clip_rect([10.5, 39.25, 30., 22.]),
            Some([0., 0., 20.25, 20.75])
        );
        assert_eq!(
            v.clip_rect([30., 50., 10., 10.]),
            Some([9.75, 9.5, 10., 10.])
        );
        for rect in [
            [0., 0., 10., 10.],
            [0., 0., 0., 100.],
            [0., 0., 100., -2.],
            [f32::NAN, 50., 10., 10.],
            [30., 50., f32::INFINITY, 10.],
        ] {
            assert!(v.clip_rect(rect).is_none());
        }
    }

    #[test]
    fn long_utf8_runs_trim_on_scalar_boundaries_and_retain_spacing() {
        let mut v = Viewport::default();
        v.resize(30, 100);
        v.set_document(200, 100);
        v.scroll_by(20., 0.);
        let text = v.text_slice("αβ🦀日語xyz", 0., 50., 11., 80.).unwrap();
        assert_eq!(text.text, "β🦀日語x");
        assert_eq!(
            (text.x, text.baseline_y, text.size_px, text.advance_px),
            (-10., 50., 11., 50.)
        );
        let clipped: Vec<_> = [0., 10., 20., 30.]
            .into_iter()
            .filter_map(|y| v.clip_rect([20., y, 10., 10.]))
            .collect();
        assert_eq!(
            clipped,
            [
                [0., 0., 10., 10.],
                [0., 10., 10., 10.],
                [0., 20., 10., 10.],
                [0., 30., 10., 10.]
            ]
        );
    }

    #[test]
    fn text_culling_handles_guards_empty_whitespace_and_invalid_geometry() {
        let v = tall_page();
        // The baseline can lie just beyond the viewport while the glyphs are visible.
        assert!(v.text_slice("edge", 0., 731., 12., 30.).is_some());
        for (text, x, baseline, size, advance) in [
            ("", 0., 10., 11., 10.),
            (" \t\n", 0., 10., 11., 30.),
            ("offscreen", 0., 800., 11., 50.),
            ("above", 0., -10., 11., 50.),
            ("right", 1400., 10., 11., 50.),
            ("left", -100., 10., 11., 50.),
            ("zero", 0., 10., 0., 30.),
            ("large", 0., 10., 1441., 30.),
            ("zero advance", 0., 10., 11., 0.),
            ("negative", 0., 10., 11., -1.),
            ("nan", f32::NAN, 10., 11., 30.),
        ] {
            assert!(
                v.text_slice(text, x, baseline, size, advance).is_none(),
                "{text}"
            );
        }
        assert!(
            Viewport::default()
                .text_slice("text", 0., 10., 11., 30.)
                .is_none()
        );
    }
}
