// Native GPU bridge. CPU work is limited to decoding guest arrays and vertices.
const GL_TRIANGLE_STRIP: u32 = 0x0005;

fn gl_assemble_triangles(mode: u32, mut indices: Vec<u32>) -> Result<Vec<u32>, &'static str> {
    match mode {
        GL_TRIANGLES => {
            // GL ignores an incomplete final primitive.
            indices.truncate(indices.len() / 3 * 3);
            Ok(indices)
        }
        GL_TRIANGLE_STRIP => {
            let mut triangles = Vec::with_capacity(indices.len().saturating_sub(2) * 3);
            for (n, tri) in indices.windows(3).enumerate() {
                // Preserve facing and the final vertex. Degenerates still advance parity.
                if n % 2 == 0 {
                    triangles.extend_from_slice(tri);
                } else {
                    triangles.extend_from_slice(&[tri[1], tri[0], tri[2]]);
                }
            }
            Ok(triangles)
        }
        _ => Err("unsupported indexed primitive topology"),
    }
}

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


struct GlGpuGeometry {
    vertices: Vec<staticgl_triangle::textured::TexturedVertex>,
    indices: Vec<u32>,
    solid: [u8; 4],
}

fn gl_gpu_geometry(
    c: &WglContext,
    memory: &impl GuestMemory,
    guest_indices: &[u32],
) -> Result<GlGpuGeometry, ProviderDispatchError> {
    const API: &str = "glDrawElements";
    // The immediate carrier currently has XYZ/UV and an opaque sampled PS.
    // Admit only state represented by that contract; never flatten perspective
    // or silently drop depth, blending, fog, alpha test or lighting.
    let allowed = (1 << GlFixedState::capability_slot(FIXED_GL_DITHER).unwrap())
        | (1 << GlFixedState::capability_slot(FIXED_GL_TEXTURE_2D).unwrap())
        | (1 << GlFixedState::capability_slot(FIXED_GL_CULL_FACE).unwrap());
    if c.fixed.enabled & !allowed != 0 {
        return Err(gl_texture_error(API, format!(
            "native GPU state bridge pending enabled=0x{:x} unsupported=0x{:x}",
            c.fixed.enabled, c.fixed.enabled & !allowed)));
    }
    if !c.vertex_array_enabled {
        return Err(gl_texture_error(API, "vertex array disabled"));
    }
    let positions = c.vertex_pointer.ok_or("missing vertex pointer")?;
    let texture = c.textures.enabled.then(|| gl_texture_draw_image(&c.textures)).transpose()?;
    let coordinates = if texture.is_some() {
        if !c.textures.coord_array_enabled { return Err(gl_texture_error(API, "texture coordinate array disabled")); }
        Some(c.textures.coord_pointer.ok_or("missing texture coordinate pointer")?)
    } else { None };
    let memory = GlArraySnapshot::new(memory, c, guest_indices);
    let mut result = GlGpuGeometry { vertices: Vec::new(), indices: Vec::new(), solid: [255; 4] };
    let mut remap = GlIndexRemap::new(guest_indices);
    let mut solid_color = None;
    let [width, height] = c.drawable_size;
    if width == 0 || height == 0 { return Err(gl_texture_error(API, "empty drawable")); }
    for &index in guest_indices {
        if let Some(mapped) = remap.get(&index) {
            result.indices.push(mapped);
            continue;
        }
        let object = gl_read_array(&memory, positions, index, [0., 0., 0., 1.])?;
        let clip = gl_transform(&c.projection_matrix, gl_transform(&c.modelview_matrix, object));
        if !clip.iter().all(|v| v.is_finite()) || clip[3] != 1.0 {
            return Err(gl_texture_error(API, "native GPU XYZW shader required for perspective draw"));
        }
        let color = if c.color_array_enabled {
            gl_read_array(&memory, c.color_pointer.ok_or("missing color pointer")?, index, [1.; 4])?
        } else { [1.; 4] };
        if let Some(image) = texture {
            if !gl_texture_primary_color_supported(image.internal, c.textures.env_mode, color) {
                return Err(gl_texture_error(API, "native GPU primary-color texture combine shader required"));
            }
        } else {
            if color[3] != 1.0 || solid_color.is_some_and(|old| old != color) {
                return Err(gl_texture_error(API, "native GPU interpolated RGBA shader required"));
            }
            solid_color = Some(color);
            result.solid = gl_rgba8(color).to_le_bytes();
        }
        let uv = if let Some(pointer) = coordinates {
            gl_transform(&c.texture_matrix, gl_read_array(&memory, pointer, index, [0., 0., 0., 1.])?)
        } else { [0., 0., 0., 1.] };
        if !uv.iter().all(|v| v.is_finite()) || uv[3] != 1.0 {
            return Err(gl_texture_error(API, "native GPU projective texture shader required"));
        }
        // Viewport mapping changes vertex coordinates, never framebuffer pixels.
        let [x, y, w, h] = c.viewport;
        let position = [
            (2. * x as f32 + (clip[0] + 1.) * w as f32) / width as f32 - 1.,
            (2. * y as f32 + (clip[1] + 1.) * h as f32) / height as f32 - 1.,
            (clip[2] + 1.) * 0.5,
        ];
        // The carrier clips against the full target, so non-full viewports
        // need the original homogeneous clipping before viewport mapping.
        if c.viewport != [0, 0, width as i32, height as i32]
            && clip[..3].iter().any(|v| !(-1.0..=1.0).contains(v)) {
            return Err(gl_texture_error(API, "native GPU viewport clipping required"));
        }
        let mapped = result.vertices.len() as u32;
        remap.insert(index, mapped);
        result.vertices.push(staticgl_triangle::textured::TexturedVertex { position, uv: [uv[0], uv[1]] });
        result.indices.push(mapped);
    }
    if c.fixed.is_enabled(FIXED_GL_CULL_FACE) {
        result.indices = result.indices.chunks_exact(3).filter(|tri| {
            let a = result.vertices[tri[0] as usize].position;
            let b = result.vertices[tri[1] as usize].position;
            let d = result.vertices[tri[2] as usize].position;
            (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]) > 0.
        }).flatten().copied().collect();
    }
    Ok(result)
}

impl XpProcess {
    fn gl_draw_gpu_static(&mut self, tid: u32, esp: u32, memory: &impl GuestMemory) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glDrawElements";
        let started = std::time::Instant::now();
        let [_, mode, count, kind, address] = arguments::<5>(memory, esp)?;
        let c = self.gl_context_mut(tid, API)?;
        if !matches!(mode, GL_TRIANGLES | GL_TRIANGLE_STRIP) || count > 1_000_000 {
            return Err(gl_texture_error(API, "unsupported native GPU topology/count"));
        }
        let item = match kind { GL_UNSIGNED_BYTE => 1, GL_UNSIGNED_SHORT => 2, GL_UNSIGNED_INT => 4,
            _ => return Err(gl_texture_error(API, "unsupported index type")) };
        if count < 3 { return Ok(0); }
        let mut raw = vec![0; count as usize * item];
        address.checked_add(raw.len() as u32 - 1).ok_or("index address overflow")?;
        memory.read(address, &mut raw)?;
        let indices = raw.chunks_exact(item).map(|b| match item {
            1 => u32::from(b[0]), 2 => u32::from(u16::from_le_bytes(b.try_into().unwrap())),
            _ => u32::from_le_bytes(b.try_into().unwrap()),
        }).collect();
        let indices = gl_assemble_triangles(mode, indices)?;
        if c.debug_draws_remaining != 0 {
            c.debug_draws_remaining -= 1;
            logl::emit(level::IMPORTANT, format_args!(
                "WC3 DEBUG GPU DRAW seq={} remaining={} indices={} vertex={:?} color={:?} uv={:?} texture={} enabled=0x{:x} viewport={:?}",
                c.draw_count + 1, c.debug_draws_remaining, indices.len(), c.vertex_pointer,
                c.color_pointer.filter(|_| c.color_array_enabled), c.textures.coord_pointer,
                c.textures.binding, c.fixed.enabled, c.viewport,
            ));
        }
        let geometry = gl_gpu_geometry(c, memory, &indices)?;
        let window = c.ui4_window_id.ok_or("GL UI4 frame missing")?;
        let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
        if !geometry.indices.is_empty() {
            if runtime.textured_renderer.is_none() {
                runtime.textured_renderer = Some(staticgl_triangle::textured::TexturedRenderer::new(runtime.device)
                    .map_err(|rc| gl_texture_error(API, format!("native GPU pipeline rc={rc}")))?);
            }
            let c = runtime.contexts.values().find(|c| c.current_tid == Some(tid)).ok_or("GL context missing")?;
            let (pixels, width, height) = if c.textures.enabled {
                let image = gl_texture_draw_image(&c.textures)?;
                (image.rgba.as_slice(), image.width, image.height)
            } else { (geometry.solid.as_slice(), 1, 1) };
            let surface = runtime.device.acquire_ui4_surface(window)
                .map_err(|rc| gl_texture_error(API, format!("native GPU acquire rc={rc}")))?;
            if [surface.info().width, surface.info().height] != c.drawable_size {
                return Err(gl_texture_error(API, "drawable/surface size mismatch"));
            }
            let point = runtime.textured_renderer.as_mut().unwrap().draw_over(runtime.queue, surface,
                &geometry.vertices, &geometry.indices, pixels, width, height)
                .map_err(|rc| gl_texture_error(API, format!("native GPU indexed submit rc={rc}")))?;
            runtime.device.wait(runtime.queue, point.value)
                .map_err(|rc| gl_texture_error(API, format!("native GPU wait rc={rc}")))?;
        }
        let c = self.gl_context_mut(tid, API)?;
        c.draw_count += 1;
        if !cfg!(feature = "nolog") {
            c.heartbeat.work.draws += 1;
            c.heartbeat.work.triangles += geometry.indices.len() as u64 / 3;
            c.heartbeat.work.draw_time += started.elapsed();
        }
        Ok(0)
    }

    fn gl_clear_gpu(&mut self, tid: u32, mask: u32) -> Result<u32, ProviderDispatchError> {
        const API: &str = "glClear";
        let c = self.gl_context_mut(tid, API)?;
        if mask == 0 { return Ok(0); }
        if mask & GL_DEPTH_BUFFER_BIT != 0 && c.fixed.depth_mask {
            return Err(gl_texture_error(API, "native GPU shared GL depth target required"));
        }
        if mask & GL_COLOR_BUFFER_BIT == 0 { return Ok(0); }
        if c.fixed.is_enabled(FIXED_GL_SCISSOR_TEST) && c.fixed.scissor != [0, 0, c.drawable_size[0] as i32, c.drawable_size[1] as i32] {
            return Err(gl_texture_error(API, "native GPU scissored clear required"));
        }
        let window = c.ui4_window_id.ok_or("GL UI4 frame missing")?;
        let color = gl_rgba8(c.clear_color);
        let runtime = self.gl_runtime.as_ref().ok_or("GL runtime missing")?;
        let surface = runtime.device.acquire_ui4_surface(window)
            .map_err(|rc| gl_texture_error(API, format!("native GPU acquire rc={rc}")))?;
        let point = runtime.device.submit_ui4_clear(runtime.queue, surface, color)
            .map_err(|rc| gl_texture_error(API, format!("native GPU clear rc={rc}")))?;
        runtime.device.wait(runtime.queue, point.value)
            .map_err(|rc| gl_texture_error(API, format!("native GPU clear wait rc={rc}")))?;
        Ok(0)
    }
}

#[cfg(test)]
mod staticgl_gpu_tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&self, address: u32, out: &mut [u8]) -> Result<(), &'static str> {
            out.copy_from_slice(self.0.get(address as usize..address as usize + out.len()).ok_or("unmapped")?);
            Ok(())
        }
        fn write(&mut self, _: u32, _: &[u8]) -> Result<(), &'static str> { panic!("draw must not write guest memory") }
    }
    fn scene() -> (WglContext, Memory) {
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.vertex_array_enabled = true;
        c.vertex_pointer = Some(GlArrayPointer { size: 3, kind: GL_FLOAT, stride: 0, address: 4 });
        let mut bytes = vec![0; 4];
        for p in [[-1f32, -1., -1.], [1., -1., 0.], [-1., 1., 1.], [1., 1., 0.]] {
            for v in p { bytes.extend_from_slice(&v.to_le_bytes()); }
        }
        (c, Memory(bytes))
    }
    #[test]
    fn indexed_strip_preserves_shared_vertices_winding_and_gl_depth_mapping() {
        let (c, memory) = scene();
        let indices = gl_assemble_triangles(GL_TRIANGLE_STRIP, vec![0, 1, 2, 3]).unwrap();
        let draw = gl_gpu_geometry(&c, &memory, &indices).unwrap();
        assert_eq!(draw.vertices.len(), 4);
        assert_eq!(draw.indices, [0, 1, 2, 2, 1, 3]);
        assert_eq!(draw.vertices[0].position, [-1., -1., 0.]);
        assert_eq!(draw.vertices[1].position, [1., -1., 0.5]);
        assert_eq!(draw.vertices[2].position, [-1., 1., 1.]);
        assert!(c.raster_frame.is_none());
    }
    #[test]
    fn gpu_bridge_does_not_flatten_perspective_or_drop_depth() {
        let (mut c, memory) = scene();
        c.projection_matrix[15] = 2.;
        assert!(gl_gpu_geometry(&c, &memory, &[0, 1, 2]).is_err());
        c.projection_matrix = GL_IDENTITY_MATRIX;
        c.fixed.set_enabled(FIXED_GL_DEPTH_TEST, true).unwrap();
        assert!(gl_gpu_geometry(&c, &memory, &[0, 1, 2]).is_err());
        assert!(c.raster_frame.is_none());
    }
    #[test]
    fn viewport_mapping_and_backface_cull_preserve_front_faces() {
        let (mut c, memory) = scene();
        c.viewport = [160, 120, 320, 240];
        c.fixed.set_enabled(FIXED_GL_CULL_FACE, true).unwrap();
        let draw = gl_gpu_geometry(&c, &memory, &[0, 1, 2, 2, 1, 0]).unwrap();
        assert_eq!(draw.indices, [0, 1, 2]);
        assert_eq!(draw.vertices[0].position, [-0.5, -0.5, 0.]);
        assert_eq!(draw.vertices[2].position, [-0.5, 0.5, 1.]);
    }
    #[test]
    fn texture_coordinates_reach_native_vertices_and_modulation_is_not_discarded() {
        let (mut c, memory) = scene();
        c.textures.enabled = true;
        c.textures.coord_array_enabled = true;
        c.textures.coord_pointer = Some(GlArrayPointer { size: 2, kind: GL_FLOAT, stride: 12, address: 4 });
        let t = c.textures.object_mut();
        t.min_filter = 0x2600;
        t.mag_filter = 0x2600;
        t.levels.insert(0, GlTextureImage { width: 1, height: 1, internal: 0x1908, rgba: vec![10, 20, 30, 255] });
        let draw = gl_gpu_geometry(&c, &memory, &[0, 1, 2]).unwrap();
        assert_eq!(draw.vertices[1].uv, [1., -1.]);
        c.color_array_enabled = true;
        c.color_pointer = c.vertex_pointer;
        assert!(gl_gpu_geometry(&c, &memory, &[0, 1, 2]).is_err());
    }
}
