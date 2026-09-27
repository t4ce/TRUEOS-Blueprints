//! UI4 producer ownership spans GL clears/draws until publication at swap.
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
pub fn needs_frame(op: ProviderOp) -> bool {
    matches!(
        op,
        ProviderOp::GlClear | ProviderOp::GlDrawElements | ProviderOp::WglSwapLayerBuffers
    )
}
pub fn state_only(name: &str) -> bool {
    name.starts_with("gl") && !matches!(name, "glClear" | "glDrawElements")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clear_draw_draw_swap_draw_has_two_acquisitions_and_one_publication() {
        let mut frames = GlFrames::default();
        let mut events = Vec::new();
        for op in [
            ProviderOp::GlClear,
            ProviderOp::GlDrawElements,
            ProviderOp::GlDrawElements,
            ProviderOp::WglSwapLayerBuffers,
            ProviderOp::GlDrawElements,
        ] {
            assert!(needs_frame(op));
            if frames.needs_begin(17) {
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
            [
                "begin", "render", "render", "render", "publish", "begin", "render"
            ]
        );
    }
    #[test]
    fn first_draw_and_empty_swap_both_need_a_producer() {
        let mut frames = GlFrames::default();
        assert!(needs_frame(ProviderOp::GlDrawElements));
        assert!(frames.needs_begin(17));
        frames.began(17);
        assert!(!frames.needs_begin(17));
        assert!(frames.needs_begin(18));
        assert!(needs_frame(ProviderOp::WglSwapLayerBuffers));
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
        assert!(!needs_frame(ProviderOp::GlClearColor));
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
