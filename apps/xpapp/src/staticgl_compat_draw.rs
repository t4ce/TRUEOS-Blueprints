// CPU fixed-function rendering; only the completed image is uploaded for UI4 publication.
include!("staticgl_cpu_reference.rs");

impl XpProcess {
    fn wgl_get_proc_address_static(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, name] = arguments::<2>(memory, esp)?;
        let _name = read_c_string(memory, name, 256)?;
        // No extensions are advertised. Static GL1.1 imports use their existing thunks.
        Ok(0)
    }
    fn wgl_delete_context_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, handle] = arguments::<2>(memory, esp)?;
        let Some(runtime) = self.gl_runtime.as_mut() else {
            return Ok(0);
        };
        let Some(c) = runtime.contexts.get(&handle) else {
            return Ok(0);
        };
        if c.current_tid.is_some_and(|owner| owner != tid) {
            return Ok(0);
        }
        runtime.contexts.remove(&handle);
        Ok(1)
    }
    fn gl_read_buffer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mode] = arguments::<2>(memory, esp)?;
        self.gl_context_mut(tid, "glReadBuffer")?;
        if mode != 0x405 {
            return Err(gl_texture_error(
                "glReadBuffer",
                format!("unsupported read buffer0x{mode:x}"),
            ));
        }
        Ok(0)
    }
    fn gl_read_pixels_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glReadPixels";
        let [_, x, y, width, height, format, kind, output] = arguments::<8>(memory, esp)?;
        let c = self.gl_context_mut(tid, API)?;
        if (width as i32) < 0 || (height as i32) < 0 {
            c.fixed.set_error(0x501);
            return Ok(0);
        }
        let channels = match format {
            0x1907 | 0x80e0 => 3,
            0x1908 | 0x80e1 => 4,
            _ => return Err(gl_texture_error(API, "unsupported readback format")),
        };
        if kind != GL_UNSIGNED_BYTE {
            return Err(gl_texture_error(API, "unsupported readback type"));
        }
        if width == 0 || height == 0 {
            return Ok(0);
        }
        if u64::from(width) * u64::from(height) > raster::MAX_RASTER_PIXELS as u64 || output == 0 {
            return Err(gl_texture_error(API, "readback budget/null output"));
        }
        Self::gl_ensure_raster(c)?;
        let pack = c.textures.pack;
        let stride_pixels = if pack.row_length == 0 {
            width
        } else {
            pack.row_length
        } as u64;
        let align = pack.alignment as u64;
        let stride = (stride_pixels * channels + align - 1) & !(align - 1);
        let first = (pack.skip_rows as u64)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(pack.skip_pixels as u64 * channels))
            .and_then(|v| v.checked_add(output as u64))
            .ok_or("readback address overflow")?;
        let end = (height as u64 - 1)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(first))
            .and_then(|v| v.checked_add(width as u64 * channels))
            .ok_or("readback address overflow")?;
        if end > u32::MAX as u64 + 1 {
            return Err(gl_texture_error(API, "readback address overflow"));
        }
        let frame = c.raster_frame.as_ref().unwrap();
        let mut row = vec![0; width as usize * channels as usize];
        for dy in 0..height {
            for dx in 0..width {
                let px = x as i32 as i64 + dx as i64;
                let py = y as i32 as i64 + dy as i64;
                let mut color = [0u8; 4];
                if px >= 0 && py >= 0 && px < frame.width as i64 && py < frame.height as i64 {
                    let start = (py as usize * frame.width as usize + px as usize) * 4;
                    color.copy_from_slice(&frame.rgba[start..start + 4]);
                }
                if matches!(format, 0x80e0 | 0x80e1) {
                    color.swap(0, 2);
                }
                let start = dx as usize * channels as usize;
                row[start..start + channels as usize].copy_from_slice(&color[..channels as usize]);
            }
            memory.write((first + dy as u64 * stride) as u32, &row)?;
        }
        Ok(0)
    }
    pub fn gl_preview_pending(&self, tid: u32) -> bool {
        cfg!(feature = "preview-first-draw")
            && self
                .gl_runtime
                .as_ref()
                .and_then(|r| r.contexts.values().find(|c| c.current_tid == Some(tid)))
                .is_some_and(|c| c.draw_count == 0)
    }
    fn gl_draw_compat_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glDrawElements";
        let draw_started = (!cfg!(feature = "nolog")).then(std::time::Instant::now);
        let [_, mode, count, kind, address] = arguments::<5>(memory, esp)?;
        if !matches!(mode, GL_TRIANGLES | GL_TRIANGLE_STRIP)
            || count > 1_000_000
            || !matches!(kind, GL_UNSIGNED_SHORT | GL_UNSIGNED_INT | GL_UNSIGNED_BYTE)
        {
            return Err(gl_texture_error(
                API,
                format!("unsupported indexed draw mode={mode} count={count} type=0x{kind:x}"),
            ));
        }
        if count < 3 {
            self.gl_context_mut(tid, API)?;
            return Ok(0);
        }
        let bytes = match kind {
            GL_UNSIGNED_BYTE => 1,
            GL_UNSIGNED_SHORT => 2,
            _ => 4,
        };
        let mut raw = vec![0; count as usize * bytes];
        if count != 0 {
            address
                .checked_add(raw.len() as u32 - 1)
                .ok_or("index address overflow")?;
            memory.read(address, &mut raw)?;
        }
        let guest_indices: Vec<u32> = raw
            .chunks_exact(bytes)
            .map(|b| match bytes {
                1 => b[0] as u32,
                2 => u16::from_le_bytes(b.try_into().unwrap()) as u32,
                _ => u32::from_le_bytes(b.try_into().unwrap()),
            })
            .collect();
        let guest_indices = gl_assemble_triangles(mode, guest_indices)?;
        let c = self.gl_context_mut(tid, API)?;
        let (stats, vertex_count) = gl_rasterize_elements(c, memory, &guest_indices)?;
        c.draw_count += 1;
        if let Some(started) = draw_started {
            let work = &mut c.heartbeat.work;
            work.draws += 1;
            work.triangles += stats.clipped_triangles as u64;
            work.pixels += stats.shaded_pixels as u64;
            work.draw_time += started.elapsed();
            if let Some(frame) = c.raster_frame.as_ref() {
                work.record_raster(frame.timing);
            }
        }
        let preview = cfg!(feature = "preview-first-draw") && c.draw_count == 1;
        if preview || c.draw_count.is_multiple_of(128) {
            logl::log!(
                level::IMPORTANT,
                format_args!(
                    "XPAPP GL RASTER DRAW tid={tid} draw={} mode=0x{mode:04x} indices={count} vertices={} triangles={} pixels={} viewport={:?} scissor={:?} enabled=0x{:x} texture={} renderer=rust-fixed",
                    c.draw_count,
                    vertex_count,
                    stats.clipped_triangles,
                    stats.shaded_pixels,
                    c.viewport,
                    c.fixed.scissor,
                    c.fixed.enabled,
                    c.textures.binding
                ),
            );
        }
        if preview {
            self.gl_present_raster(tid, "first-draw-preview")?;
        }
        Ok(0)
    }
    fn gl_present_raster(&mut self, tid: u32, reason: &str) -> Result<(), ProviderDispatchError> {
        const API: &str = "OpenGL present";
        let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
        let c = runtime
            .contexts
            .values_mut()
            .find(|c| c.current_tid == Some(tid))
            .ok_or("GL context missing")?;
        Self::gl_ensure_raster(c)?;
        let frame = c.raster_frame.as_ref().unwrap();
        let width = frame.width;
        let height = frame.height;
        let nonblack = if logl::ENABLED {
            frame
                .rgba
                .chunks_exact(4)
                .filter(|p| p[..3] != [0, 0, 0])
                .count()
        } else {
            0
        };
        let window_id = c.ui4_window_id.ok_or("GL UI4 frame missing")?;
        if runtime.textured_renderer.is_none() {
            runtime.textured_renderer = Some(
                staticgl_triangle::textured::TexturedRenderer::new(runtime.device)
                    .map_err(|e| gl_texture_error(API, format!("pipeline failed {e}")))?,
            );
        }
        // The broker needs both the upload buffer and a resident sampler copy.
        // Keep both bounded; do not allocate two full-resolution textures.
        let mut strips = 0;
        for top in (0..height).step_by(256) {
            let rows = (height - top).min(256);
            gl_fill_present_strip(&mut c.present_pixels, &frame.rgba, width, height, top, rows);
            let pixels = &c.present_pixels;
            let surface = runtime.device.acquire_ui4_surface(window_id).map_err(|e| {
                gl_texture_error(API, format!("surface acquire failed strip={top} rc={e}"))
            })?;
            let info = surface.info();
            if [info.width, info.height] != [width, height] {
                return Err(gl_texture_error(API, "drawable/surface size mismatch"));
            }
            if top == 0 {
                logl::log!(
                    level::IMPORTANT,
                    format_args!(
                        "XPAPP GL PRESENT BEGIN tid={tid} reason={reason} hwnd=0x{:08x} ui4_window={window_id} drawable={width}x{height} surface_pitch={} strip_rows=256 upload_bytes={} accounting={:?}",
                        c.hwnd,
                        info.pitch,
                        pixels.len(),
                        runtime.device.info()
                    ),
                );
            }
            let renderer = runtime.textured_renderer.as_mut().unwrap();
            let vertices = gl_present_strip_vertices(height, top, rows);
            // The owned raster frame is an opaque, full-frame publication.
            // Every strip, including the first, preserves other rows and
            // must not allocate an unrelated UI4 depth target.
            let result = renderer.draw_over(
                runtime.queue,
                surface,
                &vertices,
                &[0, 1, 2, 0, 2, 3],
                &pixels,
                width,
                rows,
            );
            let point = result.map_err(|rc| {
                let detail = format!("frame publication failed strip_top={top} rows={rows} rc={rc} stage={:?} accounting={:?}", renderer.last_failure(), runtime.device.info());
                logl::log!(level::IMPORTANT, format_args!("XPAPP GL PRESENT FAIL {detail}"));
                gl_texture_error(API, detail)
            })?;
            // Each submission consumes its surface lease. Wait before reusing
            // upload storage, then reacquire the same unpublished UI4 frame.
            runtime
                .device
                .wait(runtime.queue, point.value)
                .map_err(|e| {
                    gl_texture_error(API, format!("frame wait failed strip={top} rc={e}"))
                })?;
            strips += 1;
        }
        logl::log!(
            level::IMPORTANT,
            format_args!(
                "XPAPP GL FRAME PRESENT tid={tid} reason={reason} draws={} size={width}x{height} strips={strips} nonblack_pixels={nonblack} raster=rust-fixed gpu=completed",
                c.draw_count
            ),
        );
        Ok(())
    }
}

#[cfg(test)]
mod staticgl_compat_tests {
    use super::*;
    include!("staticgl_compat_tests.rs");
}
