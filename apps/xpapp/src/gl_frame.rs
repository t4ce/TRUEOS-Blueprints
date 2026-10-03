//! CPU drawing needs dimensions; UI4 ownership is acquired only for publication.
use crate::child_loader::ProviderOp;
use std::collections::HashSet;

#[derive(Default)]
pub struct GlFrames {
    open: HashSet<u32>,
}
impl GlFrames {
    pub fn needs_begin(&self, window: u32) -> bool {
        !self.open.contains(&window)
    }
    /// Call only after begin succeeds.
    pub fn began(&mut self, window: u32) {
        self.open.insert(window);
    }
    /// Call only after publication succeeds.
    pub fn published(&mut self, window: u32) {
        self.open.remove(&window);
    }
}
pub fn needs_drawable(op: ProviderOp) -> bool {
    matches!(
        op,
        ProviderOp::GlClear | ProviderOp::GlDrawElements | ProviderOp::WglSwapLayerBuffers
    )
}
pub fn publishes_frame(op: ProviderOp, preview_draw: bool) -> bool {
    op == ProviderOp::WglSwapLayerBuffers || (op == ProviderOp::GlDrawElements && preview_draw)
}
pub fn state_only(name: &str) -> bool {
    name.starts_with("gl") && !matches!(name, "glClear" | "glDrawElements")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_clear_and_draw_acquire_only_at_swap() {
        let mut frames = GlFrames::default();
        let mut events = Vec::new();
        for op in [
            ProviderOp::GlClear,
            ProviderOp::GlDrawElements,
            ProviderOp::GlDrawElements,
            ProviderOp::WglSwapLayerBuffers,
            ProviderOp::GlDrawElements,
        ] {
            assert!(needs_drawable(op));
            if publishes_frame(op, false) && frames.needs_begin(17) {
                events.push("begin");
                frames.began(17);
            }
            if op == ProviderOp::WglSwapLayerBuffers {
                events.push("publish");
                frames.published(17);
            } else {
                events.push("render");
            }
        }
        assert_eq!(
            events,
            ["render", "render", "render", "begin", "publish", "render"]
        );
    }
    #[test]
    fn preview_draw_and_empty_swap_need_a_producer() {
        let mut frames = GlFrames::default();
        assert!(!publishes_frame(ProviderOp::GlDrawElements, false));
        assert!(publishes_frame(ProviderOp::GlDrawElements, true));
        assert!(frames.needs_begin(17));
        frames.began(17);
        assert!(!frames.needs_begin(17));
        assert!(frames.needs_begin(18));
        assert!(publishes_frame(ProviderOp::WglSwapLayerBuffers, false));
        frames.published(17);
        assert!(frames.needs_begin(17));
    }
    #[test]
    fn clear_cannot_bypass_coordination_in_nolog_builds() {
        for name in [
            "glClear",
            "glDrawElements",
            "wglSwapLayerBuffers",
            "wglMakeCurrent",
        ] {
            assert!(!state_only(name));
        }
        for name in ["glClearColor", "glEnable", "glVertexPointer"] {
            assert!(state_only(name));
        }
        assert!(!needs_drawable(ProviderOp::GlClearColor));
    }
    #[test]
    fn failed_begin_is_retried_without_claiming_a_lease() {
        let mut frames = GlFrames::default();
        assert!(frames.needs_begin(17));
        // Busy/error: no began callback, so ownership stays unclaimed.
        assert!(frames.needs_begin(17));
        frames.began(17);
        // Failed publication: no published callback, so ownership remains held.
        assert!(!frames.needs_begin(17));
    }
}
