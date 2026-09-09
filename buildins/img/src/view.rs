#[derive(Clone, Copy, Debug)]
pub(super) enum Alignment {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) viewport_width: u32,
    pub(super) viewport_height: u32,
    pub(super) native_viewport_width: u32,
    pub(super) native_viewport_height: u32,
    pub(super) image_width: u32,
    pub(super) image_height: u32,
    pub(super) offset_x: f32,
    pub(super) offset_y: f32,
    pub(super) letterbox: bool,
    pub(super) fit_on_open: bool,
    pub(super) scale: f32,
    pub(super) zoomed: bool,
}

impl View {
    pub(super) fn new(
        viewport_width: u32,
        viewport_height: u32,
        image_width: u32,
        image_height: u32,
        alignment: Alignment,
    ) -> Self {
        let overflow_x = image_width.saturating_sub(viewport_width) as f32;
        let overflow_y = image_height.saturating_sub(viewport_height) as f32;
        let (offset_x, offset_y) = match alignment {
            Alignment::Center => (-overflow_x * 0.5, -overflow_y * 0.5),
            Alignment::TopLeft => (0.0, 0.0),
            Alignment::TopRight => (-overflow_x, 0.0),
            Alignment::BottomLeft => (0.0, -overflow_y),
            Alignment::BottomRight => (-overflow_x, -overflow_y),
        };
        let mut view = Self {
            viewport_width,
            viewport_height,
            native_viewport_width: viewport_width,
            native_viewport_height: viewport_height,
            image_width,
            image_height,
            offset_x,
            offset_y,
            letterbox: false,
            fit_on_open: false,
            scale: 1.0,
            zoomed: false,
        };
        view.clamp_offsets();
        view
    }

    pub(super) fn pan(&mut self, dx: i32, dy: i32) {
        if self.letterbox {
            return;
        }
        self.offset_x += dx as f32;
        self.offset_y += dy as f32;
        self.clamp_offsets();
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.viewport_width = width;
        self.viewport_height = height;
        if !self.zoomed {
            self.letterbox =
                self.fit_on_open || width != self.native_viewport_width || height != self.native_viewport_height;
        }
        self.clamp_offsets();
    }

    pub(super) fn zoom_at(&mut self, wheel: i16, local_x: i32, local_y: i32) -> bool {
        if wheel == 0 {
            return false;
        }
        // Resolve auto-fit into the same projection used by the fitted painter.
        let (old_scale_x, old_scale_y, old_x, old_y) = if self.letterbox {
            let (width, height) = contained_extent(
                self.image_width as usize,
                self.image_height as usize,
                self.viewport_width as usize,
                self.viewport_height as usize,
            );
            (
                width as f32 / self.image_width as f32,
                height as f32 / self.image_height as f32,
                ((self.viewport_width as usize - width) / 2) as f32,
                ((self.viewport_height as usize - height) / 2) as f32,
            )
        } else {
            (self.scale, self.scale, self.offset_x, self.offset_y)
        };
        let new_scale = (old_scale_x + wheel as f32 * 0.1).clamp(0.1, 5.0);
        if !self.letterbox && (new_scale - self.scale).abs() < 0.00001 {
            return false;
        }
        let x = local_x.clamp(0, self.viewport_width as i32) as f32;
        let y = local_y.clamp(0, self.viewport_height as i32) as f32;
        self.offset_x = x - (x - old_x) / old_scale_x * new_scale;
        self.offset_y = y - (y - old_y) / old_scale_y * new_scale;
        self.scale = new_scale;
        self.zoomed = true;
        self.letterbox = false;
        self.clamp_offsets();
        true
    }

    pub(super) fn source_at(self, x: usize, y: usize) -> Option<(usize, usize)> {
        let source_x = (x as f32 - self.offset_x) / self.scale;
        let source_y = (y as f32 - self.offset_y) / self.scale;
        if source_x < 0.0
            || source_y < 0.0
            || source_x >= self.image_width as f32
            || source_y >= self.image_height as f32
        {
            None
        } else {
            Some((source_x as usize, source_y as usize))
        }
    }

    pub(super) fn clamp_offsets(&mut self) {
        self.offset_x = clamp_axis(
            self.offset_x,
            self.viewport_width as f32,
            self.image_width as f32 * self.scale,
        );
        self.offset_y = clamp_axis(
            self.offset_y,
            self.viewport_height as f32,
            self.image_height as f32 * self.scale,
        );
    }
}

fn clamp_axis(offset: f32, viewport: f32, content: f32) -> f32 {
    if content <= viewport {
        (viewport - content) * 0.5
    } else {
        offset.clamp(viewport - content, 0.0)
    }
}

pub(super) fn contained_extent(
    source_width: usize,
    source_height: usize,
    viewport_width: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if viewport_width.saturating_mul(source_height) <= viewport_height.saturating_mul(source_width)
    {
        (
            viewport_width,
            (source_height.saturating_mul(viewport_width) / source_width).max(1),
        )
    } else {
        (
            (source_width.saturating_mul(viewport_height) / source_height).max(1),
            viewport_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_logo_uses_limiting_axis_and_survives_restore_size() {
        assert_eq!(contained_extent(1024, 1024, 1920, 1080), (1080, 1080));
        assert_eq!(contained_extent(3840, 2160, 800, 600), (800, 450));
        assert_eq!(contained_extent(320, 200, 1920, 1080), (1728, 1080));
        let mut v = View::new(1080, 1080, 1024, 1024, Alignment::Center);
        v.fit_on_open = true;
        v.letterbox = true;
        v.resize(500, 400);
        assert!(v.letterbox);
        v.resize(1080, 1080);
        assert!(v.letterbox);
        v.zoom_at(1, 540, 540);
        v.resize(500, 400);
        assert!(!v.letterbox);
    }

    #[test]
    fn wheel_clamps_and_zero_is_noop() {
        let mut v = View::new(800, 600, 2000, 1500, Alignment::Center);
        assert!(!v.zoom_at(0, 400, 300));
        assert!(v.zoom_at(i16::MAX, 400, 300));
        assert_eq!(v.scale, 5.0);
        assert!(!v.zoom_at(1, 400, 300));
        assert!(v.zoom_at(i16::MIN, 400, 300));
        assert_eq!(v.scale, 0.1);
        assert!(!v.zoom_at(-1, 400, 300));
        assert_eq!(v.source_at(0, 0), None);
        assert_eq!(v.source_at(400, 300), Some((1000, 750)));
    }

    #[test]
    fn cursor_anchor_survives_zoom_pan_and_resize() {
        let mut v = View::new(800, 600, 2000, 1500, Alignment::Center);
        let before = v.source_at(200, 150).unwrap();
        v.zoom_at(10, 200, 150);
        assert_eq!(v.source_at(200, 150), Some(before));
        v.pan(20, 30);
        assert_eq!(v.source_at(220, 180), Some(before));
        v.resize(1000, 700);
        assert_eq!(v.scale, 2.0);
        assert!(!v.letterbox);
        v.pan(i32::MAX, i32::MAX);
        assert_eq!((v.offset_x, v.offset_y), (0.0, 0.0));
        v.pan(i32::MIN, i32::MIN);
        assert_eq!((v.offset_x, v.offset_y), (-3000.0, -2300.0));
    }

    #[test]
    fn gallery_fit_transitions_to_cursor_zoom() {
        let mut v = View::new(800, 600, 4000, 3000, Alignment::Center);
        v.letterbox = true;
        v.zoom_at(1, 400, 300);
        assert!((v.scale - 0.3).abs() < 0.00001);
        let (x, y) = v.source_at(400, 300).unwrap();
        assert!(x.abs_diff(2000) <= 1 && y.abs_diff(1500) <= 1);
        assert!(!v.letterbox);
        assert!(v.zoomed);
    }
}
