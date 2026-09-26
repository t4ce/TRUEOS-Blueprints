// Fixed-function compatibility path. Raster is CPU Rust, publication is a real
// sampled vGPU submission. This deliberately keeps hardware execution separate
// from software rendering receipts.
use crate::staticgl_raster as raster;

fn gl_read_array(
    memory: &impl GuestMemory,
    pointer: GlArrayPointer,
    index: u32,
    mut value: [f32; 4],
) -> Result<[f32; 4], ProviderDispatchError> {
    let item = match pointer.kind {
        GL_FLOAT => 4,
        GL_UNSIGNED_BYTE => 1,
        _ => {
            return Err(gl_texture_error(
                "glDrawElements",
                "array component type unsupported",
            ));
        }
    };
    let bytes = pointer
        .size
        .checked_mul(item)
        .filter(|n| *n <= 16)
        .ok_or("array size invalid")?;
    let stride = if pointer.stride == 0 {
        bytes
    } else {
        pointer.stride
    };
    let address = index
        .checked_mul(stride)
        .and_then(|o| pointer.address.checked_add(o))
        .ok_or("array address overflow")?;
    let mut raw = [0u8; 16];
    memory.read(address, &mut raw[..bytes as usize])?;
    for component in 0..pointer.size as usize {
        value[component] = if item == 4 {
            f32::from_le_bytes(raw[component * 4..component * 4 + 4].try_into().unwrap())
        } else {
            raw[component] as f32 / 255.0
        };
    }
    if !value.iter().all(|v| v.is_finite()) {
        return Err(gl_texture_error("glDrawElements", "nonfinite client array"));
    }
    Ok(value)
}

fn gl_raster_compare(value: u32) -> Result<raster::Compare, &'static str> {
    use raster::Compare::*;
    Ok(match value {
        0x200 => Never,
        0x201 => Less,
        0x202 => Equal,
        0x203 => Lequal,
        0x204 => Greater,
        0x205 => NotEqual,
        0x206 => Gequal,
        0x207 => Always,
        _ => return Err("unsupported comparison"),
    })
}
fn gl_raster_blend(value: u32) -> Result<raster::BlendFactor, &'static str> {
    use raster::BlendFactor::*;
    Ok(match value {
        0 => Zero,
        1 => One,
        0x300 => SrcColor,
        0x301 => OneMinusSrcColor,
        0x302 => SrcAlpha,
        0x303 => OneMinusSrcAlpha,
        0x304 => DstAlpha,
        0x305 => OneMinusDstAlpha,
        0x306 => DstColor,
        0x307 => OneMinusDstColor,
        0x308 => SrcAlphaSaturate,
        _ => return Err("unsupported blend factor"),
    })
}
fn gl_raster_filter(value: u32) -> Result<raster::Filter, &'static str> {
    use raster::Filter::*;
    Ok(match value {
        0x2600 => Nearest,
        0x2601 => Linear,
        0x2700 => NearestMipmapNearest,
        0x2701 => LinearMipmapNearest,
        0x2702 => NearestMipmapLinear,
        0x2703 => LinearMipmapLinear,
        _ => return Err("unsupported texture filter"),
    })
}
fn gl_raster_wrap(value: u32) -> Result<raster::Wrap, &'static str> {
    Ok(match value {
        0x2901 => raster::Wrap::Repeat,
        0x2900 => raster::Wrap::Clamp,
        0x812f => raster::Wrap::ClampToEdge,
        _ => return Err("unsupported texture wrap"),
    })
}
fn gl_raster_state(c: &WglContext) -> Result<raster::GlRasterState, &'static str> {
    let s = &c.fixed;
    Ok(raster::RasterState {
        viewport: c.viewport,
        scissor_enabled: s.is_enabled(0xc11),
        scissor: s.scissor,
        cull: if s.is_enabled(0xb44) {
            raster::Cull::Back
        } else {
            raster::Cull::None
        },
        front_ccw: true,
        depth: raster::DepthState {
            enabled: s.is_enabled(0xb71),
            func: gl_raster_compare(s.depth_func)?,
            write: s.depth_mask,
            range: s.depth_range.map(|v| v as f32),
        },
        alpha: raster::AlphaState {
            enabled: s.is_enabled(0xbc0),
            func: gl_raster_compare(s.alpha_func)?,
            reference: s.alpha_ref,
        },
        blend: raster::BlendState {
            enabled: s.is_enabled(0xbe2),
            src: gl_raster_blend(s.blend_factors[0])?,
            dst: gl_raster_blend(s.blend_factors[1])?,
        },
        polygon_offset: if s.is_enabled(0x8037) {
            s.polygon_offset
        } else {
            [0.0; 2]
        },
        fog: raster::FogState {
            enabled: s.is_enabled(0xb60),
            mode: match s.fog.mode {
                0x2601 => raster::FogMode::Linear,
                0x800 => raster::FogMode::Exp,
                0x801 => raster::FogMode::Exp2,
                _ => return Err("unsupported fog mode"),
            },
            color: s.fog.color,
            density: s.fog.density,
            start: s.fog.start,
            end: s.fog.end,
        },
        tex_env: match c.textures.env_mode {
            0x2100 => raster::TexEnvMode::Modulate,
            0x2101 => raster::TexEnvMode::Decal,
            0x1e01 => raster::TexEnvMode::Replace,
            0xbe2 => raster::TexEnvMode::Blend,
            _ => return Err("unsupported texture environment"),
        },
        tex_env_color: [0.0; 4],
    })
}

fn gl_compat_vertices(
    c: &WglContext,
    memory: &impl GuestMemory,
    guest_indices: &[u32],
) -> Result<(Vec<raster::GlRasterVertex>, Vec<u32>), ProviderDispatchError> {
    const API: &str = "glDrawElements";
    if !c.vertex_array_enabled {
        return Err(gl_texture_error(API, "vertex array disabled"));
    }
    let position = c
        .vertex_pointer
        .ok_or_else(|| gl_texture_error(API, "vertex pointer absent"))?;
    let normal_matrix =
        if c.fixed.is_enabled(0xb50) || (0xc60..=0xc63).any(|cap| c.fixed.is_enabled(cap)) {
            Some(gl_normal_matrix(&c.modelview_matrix)?)
        } else {
            None
        };
    let mut vertices = Vec::new();
    let mut indices = Vec::with_capacity(guest_indices.len());
    let mut remap = HashMap::new();
    for &index in guest_indices {
        if let Some(&mapped) = remap.get(&index) {
            indices.push(mapped);
            continue;
        }
        let object = gl_read_array(memory, position, index, [0.0, 0.0, 0.0, 1.0])?;
        let eye = gl_transform(&c.modelview_matrix, object);
        let clip = gl_transform(&c.projection_matrix, eye);
        let color = if c.color_array_enabled {
            gl_read_array(
                memory,
                c.color_pointer.ok_or("color pointer absent")?,
                index,
                [1.0; 4],
            )?
        } else {
            [1.0; 4]
        };
        let mut normal = c.fixed.current_normal;
        if let Some(matrix) = normal_matrix {
            if c.fixed.normal_array_enabled {
                let n = gl_read_array(
                    memory,
                    c.fixed.normal_pointer.ok_or("normal pointer absent")?,
                    index,
                    [0.0; 4],
                )?;
                normal = [n[0], n[1], n[2]];
            }
            normal = gl_transform_normal(&matrix, normal);
            if c.fixed.is_enabled(0xba1) {
                normal = gl_vec_normalize(normal);
            }
        }
        let color = gl_lit_color(c, eye, normal, color)?;
        let mut uv = if c.textures.enabled && c.textures.coord_array_enabled {
            gl_read_array(
                memory,
                c.textures
                    .coord_pointer
                    .ok_or("texture-coordinate pointer absent")?,
                index,
                [0.0, 0.0, 0.0, 1.0],
            )?
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        for component in 0..4 {
            if !c.fixed.is_enabled(0xc60 + component as u32) {
                continue;
            }
            uv[component] = match c.fixed.texgen_mode[component].unwrap_or(0x2400) {
                0x2400 => {
                    if component < 2 {
                        eye[component]
                    } else {
                        0.0
                    }
                } // Default eye/object planes are coordinate unit vectors.
                0x2401 => {
                    if component < 2 {
                        object[component]
                    } else {
                        0.0
                    }
                }
                0x2402 if component < 2 => {
                    let e = gl_vec_normalize([eye[0], eye[1], eye[2]]);
                    let n = gl_vec_normalize(normal);
                    let dot = gl_vec_dot(e, n);
                    let r = core::array::from_fn::<_, 3, _>(|i| e[i] - 2.0 * n[i] * dot);
                    let m = 2.0 * (r[0] * r[0] + r[1] * r[1] + (r[2] + 1.0) * (r[2] + 1.0)).sqrt();
                    if m == 0.0 {
                        0.5
                    } else {
                        r[component] / m + 0.5
                    }
                }
                _ => return Err(gl_texture_error(API, "unsupported active texgen mode")),
            };
        }
        let uv = gl_transform(&c.texture_matrix, uv);
        let vertex = raster::GlRasterVertex {
            clip,
            color,
            uv,
            fog: eye[2].abs(),
        };
        let mapped = vertices.len() as u32;
        vertices.push(vertex);
        remap.insert(index, mapped);
        indices.push(mapped);
    }
    Ok((vertices, indices))
}

fn gl_rasterize_elements(
    c: &mut WglContext,
    memory: &impl GuestMemory,
    guest_indices: &[u32],
) -> Result<(raster::RasterStats, usize), ProviderDispatchError> {
    const API: &str = "glDrawElements";
    let state = gl_raster_state(c)?;
    let (vertices, indices) = gl_compat_vertices(c, memory, guest_indices)?;
    XpProcess::gl_ensure_raster(c)?;
    let object = c.textures.object();
    let mut levels = Vec::new();
    let texture = if c.textures.enabled {
        let base = object
            .levels
            .get(&0)
            .ok_or("texture level zero undefined")?;
        if base.width == 0 || base.height == 0 {
            return Err(gl_texture_error(API, "empty texture level zero"));
        }
        let max_level = if matches!(object.min_filter, 0x2600 | 0x2601) {
            0
        } else {
            31 - base.width.max(base.height).leading_zeros()
        };
        for level in 0..=max_level {
            let image = object.levels.get(&level).ok_or("incomplete mip chain")?;
            if image.internal != base.internal {
                return Err(gl_texture_error(API, "inconsistent mip base format"));
            }
            levels.push(raster::GlRasterLevel {
                width: image.width,
                height: image.height,
                rgba: &image.rgba,
            });
        }
        Some(raster::GlRasterTexture {
            levels: &levels,
            format: match base.internal {
                0x1907 => raster::TextureFormat::Rgb,
                0x1908 => raster::TextureFormat::Rgba,
                0x1906 => raster::TextureFormat::Alpha,
                0x1909 => raster::TextureFormat::Luminance,
                0x190a => raster::TextureFormat::LuminanceAlpha,
                0x8049 => raster::TextureFormat::Intensity,
                _ => {
                    return Err(gl_texture_error(
                        API,
                        "raster texture base format unsupported",
                    ));
                }
            },
            wrap_s: gl_raster_wrap(object.wrap_s)?,
            wrap_t: gl_raster_wrap(object.wrap_t)?,
            min_filter: gl_raster_filter(object.min_filter)?,
            mag_filter: gl_raster_filter(object.mag_filter)?,
        })
    } else {
        None
    };
    let stats = c.raster_frame.as_mut().unwrap().draw_triangles(
        &vertices,
        &indices,
        &state,
        texture.as_ref(),
    )?;
    Ok((stats, vertices.len()))
}

fn gl_present_strip_pixels(rgba: &[u8], width: u32, height: u32, top: u32, rows: u32) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut pixels = Vec::with_capacity(stride * rows as usize);
    for y in top..top + rows {
        let start = (height - 1 - y) as usize * stride;
        for p in rgba[start..start + stride].chunks_exact(4) {
            pixels.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
    }
    pixels
}

fn gl_present_strip_vertices(
    height: u32,
    top: u32,
    rows: u32,
) -> [staticgl_triangle::textured::TexturedVertex; 4] {
    use staticgl_triangle::textured::TexturedVertex as V;
    let upper = 1.0 - 2.0 * top as f32 / height as f32;
    let lower = 1.0 - 2.0 * (top + rows) as f32 / height as f32;
    [
        V {
            position: [-1., lower, 0.],
            uv: [0., 1.],
        },
        V {
            position: [1., lower, 0.],
            uv: [1., 1.],
        },
        V {
            position: [1., upper, 0.],
            uv: [1., 0.],
        },
        V {
            position: [-1., upper, 0.],
            uv: [0., 0.],
        },
    ]
}

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
        self.gl_runtime
            .as_ref()
            .and_then(|r| r.contexts.values().find(|c| c.current_tid == Some(tid)))
            .is_some_and(|c| c.draw_count == 0)
    }
    fn gl_ensure_raster(c: &mut WglContext) -> Result<(), ProviderDispatchError> {
        let [width, height] = c.drawable_size;
        if c.raster_frame
            .as_ref()
            .is_none_or(|f| f.width != width || f.height != height)
        {
            c.raster_frame = Some(raster::GlRasterFrame::new(width, height)?);
        }
        Ok(())
    }
    fn gl_draw_compat_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glDrawElements";
        let [_, mode, count, kind, address] = arguments::<5>(memory, esp)?;
        if mode != GL_TRIANGLES
            || count > 1_000_000
            || count % 3 != 0
            || !matches!(kind, GL_UNSIGNED_SHORT | GL_UNSIGNED_INT | GL_UNSIGNED_BYTE)
        {
            return Err(gl_texture_error(
                API,
                format!("unsupported triangle list mode={mode} count={count} type=0x{kind:x}"),
            ));
        }
        if count == 0 {
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
        let c = self.gl_context_mut(tid, API)?;
        let (stats, vertex_count) = gl_rasterize_elements(c, memory, &guest_indices)?;
        c.draw_count += 1;
        let preview = c.draw_count == 1;
        if preview || c.draw_count.is_multiple_of(128) {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 GL RASTER DRAW tid={tid} draw={} indices={count} vertices={} triangles={} pixels={} viewport={:?} scissor={:?} enabled=0x{:x} texture={} renderer=rust-fixed",
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
        let nonblack = frame
            .rgba
            .chunks_exact(4)
            .filter(|p| p[..3] != [0, 0, 0])
            .count();
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
            let pixels = gl_present_strip_pixels(&frame.rgba, width, height, top, rows);
            let surface = runtime.device.acquire_ui4_surface(window_id).map_err(|e| {
                gl_texture_error(API, format!("surface acquire failed strip={top} rc={e}"))
            })?;
            let info = surface.info();
            if [info.width, info.height] != [width, height] {
                return Err(gl_texture_error(API, "drawable/surface size mismatch"));
            }
            if top == 0 {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 GL PRESENT BEGIN tid={tid} reason={reason} hwnd=0x{:08x} ui4_window={window_id} drawable={width}x{height} surface_pitch={} strip_rows=256 upload_bytes={} accounting={:?}",
                        c.hwnd,
                        info.pitch,
                        pixels.len(),
                        runtime.device.info()
                    ),
                );
            }
            let renderer = runtime.textured_renderer.as_mut().unwrap();
            let vertices = gl_present_strip_vertices(height, top, rows);
            let result = if top == 0 {
                renderer.draw(
                    runtime.queue,
                    surface,
                    &vertices,
                    &[0, 1, 2, 0, 2, 3],
                    &pixels,
                    width,
                    rows,
                    0xff000000,
                )
            } else {
                renderer.draw_over(
                    runtime.queue,
                    surface,
                    &vertices,
                    &[0, 1, 2, 0, 2, 3],
                    &pixels,
                    width,
                    rows,
                )
            };
            let point = result.map_err(|rc| {
                let detail = format!("frame publication failed strip_top={top} rows={rows} rc={rc} stage={:?} accounting={:?}", renderer.last_failure(), runtime.device.info());
                logl::log(level::IMPORTANT, format_args!("WC3 GL PRESENT FAIL {detail}"));
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
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 GL FRAME PRESENT tid={tid} reason={reason} draws={} size={width}x{height} strips={strips} nonblack_pixels={nonblack} raster=rust-fixed gpu=completed",
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
