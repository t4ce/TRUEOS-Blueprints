// A deliberately bounded bridge to the real sampled vGPU pipeline. Upload and
// state APIs support more than this renderer; unsupported draws remain frontiers.
impl XpProcess {
    fn gl_draw_textured_static(
        &mut self,
        tid: u32,
        mode: u32,
        count: u32,
        index_kind: u32,
        indices: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glDrawElements";
        if mode != GL_TRIANGLES
            || count > 65535
            || count % 3 != 0
            || !matches!(index_kind, GL_UNSIGNED_SHORT | GL_UNSIGNED_INT)
        {
            return Err(gl_texture_error(
                API,
                "textured draw needs a bounded indexed triangle list",
            ));
        }
        if count == 0 {
            return Ok(0);
        }
        let c = self.gl_context_mut(tid, API)?;
        if c.fixed.enabled != 0 { return Err(gl_texture_error(API, "native sampled subset requires state-free draw")); }
        let image = gl_texture_draw_image(&c.textures)?;
        if !c.vertex_array_enabled || !c.textures.coord_array_enabled {
            return Err(gl_texture_error(
                API,
                "textured draw needs vertex and texture-coordinate arrays",
            ));
        }
        let positions = c
            .vertex_pointer
            .ok_or_else(|| gl_texture_error(API, "missing vertex pointer"))?;
        let coords = c
            .textures
            .coord_pointer
            .ok_or_else(|| gl_texture_error(API, "missing texture-coordinate pointer"))?;
        let window = c
            .ui4_window_id
            .ok_or_else(|| gl_texture_error(API, "no UI4 frame"))?;
        let viewport = c.viewport;
        let texture_size = (image.width, image.height);
        let internal = image.internal;
        let mut vertices = Vec::with_capacity(count as usize);
        let stride = if index_kind == GL_UNSIGNED_SHORT {
            2
        } else {
            4
        };
        for element in 0..count {
            let address = indices
                .checked_add(element * stride)
                .ok_or("GL index address overflow")?;
            let index = if stride == 2 {
                let mut bytes = [0; 2];
                memory.read(address, &mut bytes)?;
                u32::from(u16::from_le_bytes(bytes))
            } else {
                read_u32(memory, address)?
            };
            let mut position = [0.0, 0.0, 0.0, 1.0];
            for component in 0..positions.size {
                position[component as usize] =
                    gl_array_component(memory, positions, index, component)?;
            }
            let position = gl_transform(
                &c.projection_matrix,
                gl_transform(&c.modelview_matrix, position),
            );
            // The carrier shader has XYZ, not XYZW. Do not lose perspective
            // interpolation or pretend to implement homogeneous clipping.
            if position[3] != 1.0
                || !position[..3]
                    .iter()
                    .all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
            {
                return Err(gl_texture_error(
                    API,
                    "textured draw needs in-bounds affine clip positions (w=1)",
                ));
            }
            let mut uv = [0.0, 0.0, 0.0, 1.0];
            for component in 0..coords.size {
                uv[component as usize] = gl_array_component(memory, coords, index, component)?;
            }
            let uv = gl_transform(&c.texture_matrix, uv);
            if uv[3] != 1.0 || !uv.iter().all(|v| v.is_finite()) {
                return Err(gl_texture_error(
                    API,
                    "projective/nonfinite texture coordinates unsupported",
                ));
            }
            let mut primary_color = [1.0; 4];
            if c.color_array_enabled {
                let colors = c
                    .color_pointer
                    .ok_or_else(|| gl_texture_error(API, "missing color pointer"))?;
                for component in 0..colors.size {
                    primary_color[component as usize] =
                        gl_array_component(memory, colors, index, component)?;
                }
            }
            if !gl_texture_primary_color_supported(internal, c.textures.env_mode, primary_color) {
                return Err(gl_texture_error(
                    API,
                    "texture environment needs primary-color/alpha shader support",
                ));
            }
            vertices.push(staticgl_triangle::textured::TexturedVertex {
                position: [position[0], position[1], (position[2] + 1.0) * 0.5],
                uv: [uv[0], uv[1]],
            });
        }
        let indices: Vec<u32> = (0..count).collect();
        let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
        let surface = runtime
            .device
            .acquire_ui4_surface(window)
            .map_err(|e| gl_texture_error(API, format!("surface acquire failed code={e}")))?;
        let info = surface.info();
        if viewport != [0, 0, info.width as i32, info.height as i32] {
            return Err(gl_texture_error(
                API,
                "textured draw requires a full-surface viewport",
            ));
        }
        if runtime.textured_renderer.is_none() {
            runtime.textured_renderer = Some(
                staticgl_triangle::textured::TexturedRenderer::new(runtime.device).map_err(
                    |e| gl_texture_error(API, format!("sampled pipeline create failed code={e}")),
                )?,
            );
        }
        let context = runtime
            .contexts
            .values()
            .find(|c| c.current_tid == Some(tid))
            .ok_or("GL context missing")?;
        let image = &context.textures.object().levels[&0];
        let point = runtime
            .textured_renderer
            .as_mut()
            .unwrap()
            .draw_over(
                runtime.queue,
                surface,
                &vertices,
                &indices,
                &image.rgba,
                texture_size.0,
                texture_size.1,
            )
            .map_err(|e| gl_texture_error(API, format!("sampled submit failed code={e}")))?;
        runtime
            .device
            .wait(runtime.queue, point.value)
            .map_err(|e| gl_texture_error(API, format!("sampled wait failed code={e}")))?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 GL TEXTURED DRAW tid={tid} indices={count} texture={} size={}x{} gpu=completed",
                context.textures.binding, texture_size.0, texture_size.1
            ),
        );
        Ok(0)
    }
}

fn gl_texture_draw_image(textures: &GlTextures) -> Result<&GlTextureImage, ProviderDispatchError> {
    const API: &str = "glDrawElements";
    let object = textures.object();
    if object.min_filter != 0x2600
        || object.mag_filter != 0x2600
        || object.wrap_s != 0x2901
        || object.wrap_t != 0x2901
    {
        return Err(gl_texture_error(
            API,
            format!(
                "sampled GPU needs nearest/repeat; min=0x{:x} mag=0x{:x} wrap=0x{:x}/0x{:x}",
                object.min_filter, object.mag_filter, object.wrap_s, object.wrap_t
            ),
        ));
    }
    let image = object
        .levels
        .get(&0)
        .ok_or_else(|| gl_texture_error(API, "texture level zero undefined"))?;
    if image.width == 0 || image.height == 0 || !matches!(image.internal, 0x1907 | 0x1908) {
        return Err(gl_texture_error(
            API,
            "sampled GPU needs a nonempty RGB/RGBA image",
        ));
    }
    if !image.rgba.chunks_exact(4).all(|p| p[3] == 255) {
        return Err(gl_texture_error(
            API,
            "sampled GPU currently requires opaque texture pixels",
        ));
    }
    if !matches!(textures.env_mode, 0x2100 | 0x2101 | 0x1e01) {
        return Err(gl_texture_error(
            API,
            "texture environment needs additional shader combine support",
        ));
    }
    Ok(image)
}

// Called only for the admitted opaque RGB/RGBA texture. GL 1.1 tables 3.10/3.11:
// RGB REPLACE and either DECAL preserve primary alpha; RGBA REPLACE does not.
fn gl_texture_primary_color_supported(internal: u32, env: u32, color: [f32; 4]) -> bool {
    if !matches!(internal, 0x1907 | 0x1908) || !color.iter().all(|v| v.is_finite()) {
        return false;
    }
    match env {
        0x2100 => color == [1.0; 4],
        0x1e01 if internal == 0x1908 => true,
        0x1e01 | 0x2101 => color[3] == 1.0,
        _ => false,
    }
}
