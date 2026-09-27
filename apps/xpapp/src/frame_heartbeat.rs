use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct FrameWork {
    pub draws: u64,
    pub triangles: u64,
    pub draw_time: Duration,
    pub decode_time: Duration,
    pub atlas_time: Duration,
    pub acquire_time: Duration,
    pub submit_time: Duration,
    pub wait_time: Duration,
    pub texture_hits: u64,
    pub texture_uploads: u64,
    pub texture_upload_bytes: u64,
}

pub(crate) struct Heartbeat {
    started: Instant,
    frame_started: Instant,
    last_report: Option<Instant>,
    swaps: u64,
    pub work: FrameWork,
}

impl Heartbeat {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            frame_started: now,
            last_report: None,
            swaps: 0,
            work: FrameWork::default(),
        }
    }

    pub fn published(&mut self, now: Instant, swap_time: Duration, hwnd: u32) {
        let work = core::mem::take(&mut self.work);
        let frame_time = now.duration_since(self.frame_started);
        self.frame_started = now;
        self.swaps += 1;
        if self
            .last_report
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
        {
            return;
        }
        self.last_report = Some(now);
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME hwnd=0x{hwnd:08x} swap={} draws={} submitted_triangles={} renderer=native-gpu gpu=completed ui4=published",
                self.swaps, work.draws, work.triangles,
            ),
        );
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME TIME draw_ms={:.3} swap_ms={:.3} frame_ms={:.3} context_s={:.3}",
                work.draw_time.as_secs_f64() * 1000.0,
                swap_time.as_secs_f64() * 1000.0,
                frame_time.as_secs_f64() * 1000.0,
                now.duration_since(self.started).as_secs_f64(),
            ),
        );
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!("XPAPP FRAME TEXTURES cache_hits={} uploads={} upload_bytes={}",
                work.texture_hits, work.texture_uploads, work.texture_upload_bytes),
        );
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME DRAW decode_ms={:.3} atlas_ms={:.3} acquire_ms={:.3} submit_ms={:.3} wait_ms={:.3} outside_draw_swap_ms={:.3}",
                work.decode_time.as_secs_f64() * 1000.0,
                work.atlas_time.as_secs_f64() * 1000.0,
                work.acquire_time.as_secs_f64() * 1000.0,
                work.submit_time.as_secs_f64() * 1000.0,
                work.wait_time.as_secs_f64() * 1000.0,
                frame_time.saturating_sub(work.draw_time).saturating_sub(swap_time).as_secs_f64() * 1000.0,
            ),
        );
    }
}
