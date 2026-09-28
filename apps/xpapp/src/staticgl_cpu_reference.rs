// CPU fixed-function renderer and completed-frame presentation helpers.
// Restored from the production CPU path preceding commit 1a99d723.
use crate::staticgl_raster as raster;

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
    let memory = &GlArraySnapshot::new(memory, c, guest_indices);
    let mut vertices = Vec::with_capacity(guest_indices.len().min(65536));
    let mut indices = Vec::with_capacity(guest_indices.len());
    let mut remap = GlIndexRemap::new(guest_indices);
    for &index in guest_indices {
        if let Some(mapped) = remap.get(&index) {
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
    let timing_started = (c.debug_draws_remaining != 0).then(std::time::Instant::now);
    let mut state = gl_raster_state(c)?;
    // Explicit per-texture A/B experiment. Never mutate GL state or the stored
    // depth buffer while bypassed; restoring resumes the guest's own settings.
    if c.textures.enabled && c.debug_depth_texture == Some(c.textures.binding) {
        state.depth.enabled = false;
        state.depth.write = false;
    }
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
    let decoded_at = timing_started.map(|_| std::time::Instant::now());
    // Explicit, bounded observation only. No texture scan or formatting on the
    // normal release path; a zero budget also cancels an unfinished capture.
    let capture = c.debug_draws_remaining != 0;
    if capture {
        c.debug_draws_remaining -= 1;
        logl::emit(level::IMPORTANT, format_args!(
            "XPAPP DEBUG DRAW seq={} remaining={} vertex={:?} color_array={} color={:?} uv={:?} indices={} lighting={} material_mode=0x{:x} ambient={:?} state={:?}",
            c.draw_count + 1, c.debug_draws_remaining, c.vertex_pointer,
            c.color_array_enabled, c.color_pointer, c.textures.coord_pointer,
            indices.len(), c.fixed.is_enabled(0xb50), c.fixed.color_material_mode,
            c.light_model_ambient, state,
        ));
        let raw_color = guest_indices.first().and_then(|&index| {
            c.color_pointer.filter(|_| c.color_array_enabled)
                .and_then(|pointer| gl_read_array(memory, pointer, index, [1.0; 4]).ok())
        });
        let atlas = texture.as_ref().map(|t| {
            let base = t.levels[0];
            let first_covered = base.rgba.chunks_exact(4).find(|p| p[3] != 0);
            (base.width, base.height, t.format, t.min_filter, t.mag_filter, first_covered)
        });
        logl::emit(level::IMPORTANT, format_args!(
            "XPAPP DEBUG DRAW INPUT seq={} texture={} atlas={:?} raw_color={:?} vertices={:?}",
            c.draw_count + 1, c.textures.binding, atlas, raw_color,
            &vertices[..vertices.len().min(3)],
        ));
    }
    let raster_started = timing_started.map(|_| std::time::Instant::now());
    #[cfg(all(feature = "gpu-raster", not(test)))]
    let stats = {
        let frame = c.raster_frame.as_mut().unwrap();
        let stats = frame.prepare_indexed(&state, texture, &vertices, &indices)
            .map_err(|e| gl_texture_error(API, format!("prepared geometry: {e:?}")))?;
        let (pixels, width, height) = if let Some(texture) = texture {
            gl_gpu_texture_atlas(&c.textures, texture.levels.len() as u32)?
        } else { (std::sync::Arc::<[u8]>::from([255u8;4]), 1, 1) };
        if let Some(draw) = crate::staticgl_prepared::draw(c.drawable_size,
            frame.prepared_triangles(), &state, texture, pixels, [width,height])? {
            if c.prepared_draws.len() >= trueos::vgpu::MAX_PREPARED_RASTER_DRAWS {
                return Err(gl_texture_error(API, "prepared frame draw limit"));
            }
            c.prepared_draws.push(draw);
        }
        stats
    };
    #[cfg(any(not(feature = "gpu-raster"), test))]
    let stats = c.raster_frame.as_mut().unwrap().draw_triangles(
        &vertices,
        &indices,
        &state,
        texture.as_ref(),
    )?;
    let raster_elapsed = raster_started.map(|started| started.elapsed());
    if capture {
        logl::emit(level::IMPORTANT, format_args!(
            "XPAPP DEBUG DRAW RESULT seq={} stats={stats:?}", c.draw_count + 1,
        ));
        if let (Some(started), Some(decoded_at), Some(raster_elapsed)) =
            (timing_started, decoded_at, raster_elapsed)
        {
            logl::emit(level::IMPORTANT, format_args!(
                "XPAPP DEBUG CPU COST seq={} texture={} decode_ms={:.3} raster_ms={:.3}",
                c.draw_count + 1, c.textures.binding,
                decoded_at.duration_since(started).as_secs_f64() * 1000.0,
                raster_elapsed.as_secs_f64() * 1000.0,
            ));
        }
    }
    Ok((stats, vertices.len()))
}

fn gl_fill_present_strip(
    pixels: &mut Vec<u8>,
    rgba: &[u8],
    width: u32,
    height: u32,
    top: u32,
    rows: u32,
) {
    let stride = width as usize * 4;
    pixels.resize(stride * rows as usize, 0);
    for (destination, y) in pixels.chunks_exact_mut(stride).zip(top..top + rows) {
        let start = (height - 1 - y) as usize * stride;
        destination.copy_from_slice(&rgba[start..start + stride]);
        for p in destination.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
}

#[cfg(test)]
fn gl_present_strip_pixels(rgba: &[u8], width: u32, height: u32, top: u32, rows: u32) -> Vec<u8> {
    let mut pixels = Vec::new();
    gl_fill_present_strip(&mut pixels, rgba, width, height, top, rows);
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
    fn gl_ensure_raster(c: &mut WglContext) -> Result<(), ProviderDispatchError> {
        let [width, height] = c.drawable_size;
        if c.raster_frame
            .as_ref()
            .is_none_or(|f| f.width != width || f.height != height)
        {
            #[cfg(feature = "gpu-raster")]
            if !c.prepared_draws.is_empty() { return Err("resize with pending prepared draws".into()); }
            c.raster_frame = Some(raster::GlRasterFrame::new(width, height)?);
            crate::logl::emit(crate::logl::level::IMPORTANT, format_args!(
                "XPAPP RASTER ALLOC hwnd=0x{:08x} framebuffer={}x{} pixels={} rgba_bytes={} depth_bytes={} viewport={:?} source=guest-window-drawable",
                c.hwnd, width, height, u64::from(width) * u64::from(height),
                u64::from(width) * u64::from(height) * 4,
                u64::from(width) * u64::from(height) * 4, c.viewport));
        }
        Ok(())
    }
}
