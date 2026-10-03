use std::time::{Duration, Instant};

// Coarse draw boundaries only: no clock calls inside the vertex loop.
#[derive(Default)]
pub(crate) struct DecodeTiming {
    pub index: Duration,
    pub vertex_setup: Duration,
    pub snapshot: Duration,
    pub allocation: Duration,
    pub vertices: Duration,
    pub texture: Duration,
    pub snapshot_bytes: u64,
    pub snapshot_ranges: u64,
    pub input_indices: u64,
    pub unique_vertices: u64,
    pub lit_vertices: u64,
    pub texgen_vertices: u64,
}
impl DecodeTiming {
    pub fn measured(&self) -> Duration {
        self.index
            + self.vertex_setup
            + self.snapshot
            + self.allocation
            + self.vertices
            + self.texture
    }
}

#[derive(Default)]
pub(crate) struct FrameWork {
    pub decode: DecodeTiming,
    pub draws: u64,
    pub skipped_draws: u64,
    pub clear_time: Duration,
    pub triangles: u64,
    pub pixels: u64,
    pub draw_time: Duration,
    pub raster: crate::staticgl_raster::DrawTiming,
    pub parallel_draws: u64,
    pub scalar_draws: u64,
    pub decode_time: Duration,
    pub atlas_time: Duration,
    pub acquire_time: Duration,
    pub submit_time: Duration,
    pub wait_time: Duration,
    pub texture_hits: u64,
    pub texture_uploads: u64,
    pub texture_upload_bytes: u64,
}

impl FrameWork {
    pub fn record_raster(&mut self, t: crate::staticgl_raster::DrawTiming) {
        self.parallel_draws += u64::from(t.parallel);
        self.scalar_draws +=
            u64::from(!t.parallel && !cfg!(all(feature = "gpu-raster", not(test))));
        let r = &mut self.raster;
        r.total += t.total;
        r.prepare += t.prepare;
        r.capacity += t.capacity;
        r.copy_in += t.copy_in;
        r.copy_out += t.copy_out;
        r.scalar += t.scalar;
        r.texture_bytes += t.texture_bytes;
        r.framebuffer_bytes += t.framebuffer_bytes;
        r.pool.submit += t.pool.submit;
        r.pool.join += t.pool.join;
        r.pool.retries += t.pool.retries;
        r.pool.polls += t.pool.polls;
        for i in 0..2 {
            r.pool.active[i] += t.pool.active[i];
            r.pool.queue[i] += t.pool.queue[i];
            r.pool.gaps[i] += t.pool.gaps[i];
            r.pool.steps[i] += t.pool.steps[i];
        }
    }
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

    pub fn published(
        &mut self,
        now: Instant,
        swap_time: Duration,
        hwnd: u32,
        raster_size: [u32; 2],
        drawable: [u32; 2],
        viewport: [i32; 4],
    ) {
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
                "XPAPP FRAME hwnd=0x{hwnd:08x} swap={} draws={} raster_triangles={} shaded_pixels={} renderer={} presentation=gpu-completed ui4=published skipped_draws={}",
                self.swaps,
                work.draws,
                work.triangles,
                work.pixels,
                if cfg!(feature = "gpu-raster") {
                    "cpu-geometry-gpu-raster"
                } else {
                    "cpu-rust-fixed"
                },
                work.skipped_draws,
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
            format_args!(
                "XPAPP FRAME CPU clear_ms={:.3} outside_draw_swap_ms={:.3}",
                work.clear_time.as_secs_f64() * 1000.0,
                frame_time
                    .saturating_sub(work.draw_time)
                    .saturating_sub(swap_time)
                    .as_secs_f64()
                    * 1000.0,
            ),
        );
        let r = work.raster;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME RASTER hwnd=0x{hwnd:08x} swap={} framebuffer={}x{} drawable={}x{} viewport={:?} framebuffer_pixels={} parallel_draws={} scalar_draws={} trace_execution={} pool_enabled={}",
                self.swaps,
                raster_size[0],
                raster_size[1],
                drawable[0],
                drawable[1],
                viewport,
                u64::from(raster_size[0]) * u64::from(raster_size[1]),
                work.parallel_draws,
                work.scalar_draws,
                cfg!(feature = "trace-execution"),
                cfg!(feature = "raster-pool")
            ),
        );
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME DRAW hwnd=0x{hwnd:08x} swap={} decode_setup_ms={:.3} clip_validate_ms={:.3} capacity_ms={:.3} copy_in_ms={:.3} submit_ms={:.3} join_ms={:.3} copy_out_ms={:.3} scalar_ms={:.3} raster_other_ms={:.3} framebuffer_copy_bytes={} texture_copy_bytes={} retries={} join_polls={} scope=draw-wall-partition",
                self.swaps,
                ms(work.draw_time.saturating_sub(r.total)),
                ms(r.prepare),
                ms(r.capacity),
                ms(r.copy_in),
                ms(r.pool.submit),
                ms(r.pool.join),
                ms(r.copy_out),
                ms(r.scalar),
                ms(r.total.saturating_sub(
                    r.prepare
                        + r.capacity
                        + r.copy_in
                        + r.pool.submit
                        + r.pool.join
                        + r.copy_out
                        + r.scalar
                )),
                r.framebuffer_bytes,
                r.texture_bytes,
                r.pool.retries,
                r.pool.polls
            ),
        );
        let d = &work.decode;
        let decode_total = work.draw_time.saturating_sub(r.total);
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!(
                "XPAPP FRAME DECODE hwnd=0x{hwnd:08x} swap={} index_ms={:.3} vertex_setup_ms={:.3} snapshot_ms={:.3} allocation_remap_ms={:.3} vertex_loop_ms={:.3} texture_frame_setup_ms={:.3} residual_ms={:.3} accounting_excess_ms={:.3} snapshot_bytes={} snapshot_ranges={} indices={} unique_vertices={} lit_vertices={} texgen_vertices={} scope=draw-wall-partition-includes-descheduling",
                self.swaps,
                ms(d.index),
                ms(d.vertex_setup),
                ms(d.snapshot),
                ms(d.allocation),
                ms(d.vertices),
                ms(d.texture),
                ms(decode_total.saturating_sub(d.measured())),
                ms(d.measured().saturating_sub(decode_total)),
                d.snapshot_bytes,
                d.snapshot_ranges,
                d.input_indices,
                d.unique_vertices,
                d.lit_vertices,
                d.texgen_vertices
            ),
        );
        if work.parallel_draws != 0 {
            for i in 0..2 {
                crate::logl::emit(
                    crate::logl::level::IMPORTANT,
                    format_args!(
                        "XPAPP FRAME BAND hwnd=0x{hwnd:08x} swap={} band={} steps={} step_wall_ms={:.3} admission_to_first_step_ms={:.3} between_steps_ms={:.3} scope=overlaps-submit-join-and-other-band step-wall-includes-descheduling gaps-include-duty-and-scheduling",
                        self.swaps,
                        i,
                        r.pool.steps[i],
                        ms(r.pool.active[i]),
                        ms(r.pool.queue[i]),
                        ms(r.pool.gaps[i])
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_timing_keeps_overlapping_band_work_out_of_draw_partition() {
        let mut work = FrameWork::default();
        let mut draw = crate::staticgl_raster::DrawTiming::default();
        draw.parallel = true;
        draw.total = Duration::from_millis(10);
        draw.pool.join = Duration::from_millis(8);
        draw.pool.active = [Duration::from_millis(6); 2];
        work.record_raster(draw);
        draw.parallel = false;
        draw.pool = Default::default();
        draw.scalar = Duration::from_millis(10);
        work.record_raster(draw);
        assert_eq!((work.parallel_draws, work.scalar_draws), (1, 1));
        assert_eq!(work.raster.total, Duration::from_millis(20));
        assert_eq!(work.raster.pool.join, Duration::from_millis(8));
        assert_eq!(work.raster.pool.active, [Duration::from_millis(6); 2]);
    }
}
