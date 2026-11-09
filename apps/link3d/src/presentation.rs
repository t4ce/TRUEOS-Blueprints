//! UI4's layered-window publication barrier applies even to a hidden layer.
//! Both fronts must exist before the window is visible, and both replacements
//! must publish before a resize commits. A ready replacement cannot begin again
//! until its partner publishes, including when that partner needs a retry.

pub struct LayerPublications {
    background_missing: bool,
    resize_pending: bool,
}
impl Default for LayerPublications {
    fn default() -> Self {
        Self {
            background_missing: true,
            resize_pending: false,
        }
    }
}
impl LayerPublications {
    pub fn resized(&mut self) {
        self.background_missing = true;
        self.resize_pending = true;
    }
    pub fn needs_background(&self, animated: bool) -> bool {
        self.background_missing || (animated && !self.resize_pending)
    }
    pub fn background_published(&mut self) {
        self.background_missing = false;
    }
    pub fn foreground_published(&mut self) {
        self.resize_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Models the broker's visibility/paired-resize barrier, not GPU pixels.
    #[derive(Default)]
    struct Pair {
        background_ready: bool,
        foreground_ready: bool,
        visible: bool,
        resizing: bool,
    }
    impl Pair {
        fn publish_background(&mut self) {
            assert!(
                !self.resizing || !self.background_ready,
                "ready replacement cannot begin again"
            );
            self.background_ready = true;
            self.commit();
        }
        fn publish_foreground(&mut self) {
            self.foreground_ready = true;
            self.commit();
        }
        fn commit(&mut self) {
            if self.background_ready && self.foreground_ready {
                self.visible = true;
                self.resizing = false;
            }
        }
        fn resize(&mut self) {
            self.background_ready = false;
            self.foreground_ready = false;
            self.resizing = true;
        }
    }

    #[test]
    fn transparent_background_admits_outline_only_startup() {
        let mut lifecycle = LayerPublications::default();
        let mut pair = Pair::default();
        // No PBR/reveal rendering is active; a transparent front is still required.
        assert!(lifecycle.needs_background(false));
        pair.publish_background();
        lifecycle.background_published();
        assert!(!pair.visible);
        pair.publish_foreground();
        lifecycle.foreground_published();
        assert!(pair.visible);
        assert!(!lifecycle.needs_background(false));
    }

    #[test]
    fn resize_after_intro_commits_even_though_background_is_hidden_and_idle() {
        let mut lifecycle = LayerPublications::default();
        let mut pair = Pair::default();
        for _ in 0..3 {
            // Fullscreen, restore and fullscreen again.
            lifecycle.resized();
            pair.resize();
            assert!(lifecycle.needs_background(false));
            pair.publish_background();
            lifecycle.background_published();
            assert!(pair.resizing);
            pair.publish_foreground();
            lifecycle.foreground_published();
            assert!(!pair.resizing);
            assert!(!lifecycle.needs_background(false));
        }
    }

    #[test]
    fn foreground_busy_during_reveal_resize_does_not_reacquire_ready_background() {
        let mut lifecycle = LayerPublications::default();
        let mut pair = Pair::default();
        lifecycle.resized();
        pair.resize();
        assert!(lifecycle.needs_background(true));
        pair.publish_background();
        lifecycle.background_published();
        for _ in 0..4 {
            // Foreground begin/render/sprite admission can reject Busy. Retry
            // that producer while leaving its already-ready partner untouched.
            assert!(!lifecycle.needs_background(true));
            assert!(pair.resizing);
        }
        pair.publish_foreground();
        lifecycle.foreground_published();
        assert!(!pair.resizing);
        assert!(lifecycle.needs_background(true));
        pair.publish_background();
        lifecycle.background_published();
    }
}
