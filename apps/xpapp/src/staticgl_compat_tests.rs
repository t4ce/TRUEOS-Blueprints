struct ArrayMemory(Vec<u8>);

struct CountingArrayMemory {
    memory: ArrayMemory,
    reads: core::cell::Cell<usize>,
    reject_bulk: bool,
}
impl GuestMemory for CountingArrayMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
        self.reads.set(self.reads.get() + 1);
        if self.reject_bulk && output.len() > 16 {
            return Err("unmapped stride gap");
        }
        self.memory.read(address, output)
    }
    fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
        self.memory.write(address, input)
    }
}

#[test]
fn interleaved_mesh_batches_reads_and_preserves_sparse_fallback() {
    let (c, source) = scene();
    let mut bytes = vec![0; 4];
    for _ in 0..341 {
        bytes.extend_from_slice(&source.0[4..]);
    }
    let memory = CountingArrayMemory {
        memory: ArrayMemory(bytes),
        reads: core::cell::Cell::new(0),
        reject_bulk: false,
    };
    let indices: Vec<u32> = (0..1023).collect();
    let mut timing = crate::frame_heartbeat::DecodeTiming::default();
    let (vertices, mapped) = gl_compat_vertices_timed(&c, &memory, &indices, &mut timing).unwrap();
    assert_eq!(timing.input_indices, 1023);
    assert_eq!(timing.unique_vertices, 1023);
    if !cfg!(feature = "nolog") && !cfg!(feature = "replay-arrays") {
        assert_eq!(timing.snapshot_ranges, 1);
        assert!(timing.snapshot_bytes > 0);
    }
    assert_eq!(vertices.len(), 1023);
    assert_eq!(mapped, indices);
    assert_eq!(
        memory.reads.get(),
        if cfg!(feature = "replay-arrays") {
            3069
        } else {
            1
        }
    );
    // A new draw observes new guest bytes; no address-only cross-frame cache.
    let mut memory = memory;
    memory.memory.0[4..8].copy_from_slice(&0.25f32.to_le_bytes());
    let (changed, _) = gl_compat_vertices(&c, &memory, &indices).unwrap();
    assert_ne!(changed[0].clip, vertices[0].clip);
    memory.reject_bulk = true;
    let (fallback, _) = gl_compat_vertices(&c, &memory, &indices).unwrap();
    for (a, b) in fallback.iter().zip(&changed) {
        assert_eq!(a.clip, b.clip);
        assert_eq!(a.color, b.color);
        assert_eq!(a.uv, b.uv);
    }
}
impl GuestMemory for ArrayMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
        output.copy_from_slice(
            self.0
                .get(address as usize..address as usize + output.len())
                .ok_or("unmapped vertex")?,
        );
        Ok(())
    }
    fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
        self.0
            .get_mut(address as usize..address as usize + input.len())
            .ok_or("unmapped write")?
            .copy_from_slice(input);
        Ok(())
    }
}

fn scene() -> (WglContext, ArrayMemory) {
    let mut c = WglContext::new(1, 2, 1);
    c.drawable_size = [16, 16];
    c.viewport = [0, 0, 16, 16];
    c.projection_matrix = [
        1.,
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
        0.,
        0.,
        0.,
        -11. / 9.,
        -1.,
        0.,
        0.,
        -20. / 9.,
        0.,
    ];
    c.vertex_array_enabled = true;
    c.vertex_pointer = Some(GlArrayPointer {
        size: 3,
        kind: GL_FLOAT,
        stride: 36,
        address: 4,
    });
    c.fixed.normal_array_enabled = true;
    c.fixed.normal_pointer = Some(GlArrayPointer {
        size: 3,
        kind: GL_FLOAT,
        stride: 36,
        address: 16,
    });
    c.textures.enabled = true;
    c.textures.coord_array_enabled = true;
    c.textures.coord_pointer = Some(GlArrayPointer {
        size: 2,
        kind: GL_FLOAT,
        stride: 36,
        address: 32,
    });
    for cap in [0xb50, 0x4003, 0xb60, 0xb71, 0xb44, 0xc11] {
        c.fixed.set_enabled(cap, true).unwrap();
    }
    c.fixed.scissor = [0, 0, 16, 16];
    c.light_model_ambient = [0.; 4];
    c.fixed.materials[0].diffuse = [1.; 4];
    c.fixed.lights[3].diffuse = [1.; 4];
    c.fixed.fog.mode = 0x2601;
    c.fixed.fog.start = 0.;
    c.fixed.fog.end = 4.;
    c.fixed.fog.color = [0., 0., 1., 1.];
    c.textures.bind(1).unwrap();
    for (level, width) in [(0, 2), (1, 1)] {
        c.textures
            .set_image(
                level,
                GlTextureImage {
                    width,
                    height: width,
                    internal: 0x1907,
                    rgba: [128, 64, 32, 255].repeat((width * width) as usize),
                },
            )
            .unwrap();
    }
    c.textures.object_mut().min_filter = 0x2703;
    c.textures.object_mut().mag_filter = 0x2601;
    let mut memory = ArrayMemory(vec![0; 4 + 36 * 3]);
    for (i, (position, uv)) in [
        ([-1f32, -1., -2.], [0f32, 0.]),
        ([1., -1., -2.], [1., 0.]),
        ([0., 1., -2.], [0.5, 1.]),
    ]
    .into_iter()
    .enumerate()
    {
        let start = 4 + i * 36;
        for (j, value) in position.into_iter().chain([0., 0., 1.]).enumerate() {
            memory.0[start + j * 4..start + j * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        memory.0[start + 24..start + 28].copy_from_slice(&[255; 4]);
        for (j, value) in uv.into_iter().enumerate() {
            memory.0[start + 28 + j * 4..start + 32 + j * 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    (c, memory)
}

fn pixel(c: &WglContext, x: usize, y: usize) -> [u8; 4] {
    let frame = c.raster_frame.as_ref().unwrap();
    frame.rgba[(y * frame.width as usize + x) * 4..(y * frame.width as usize + x) * 4 + 4]
        .try_into()
        .unwrap()
}

#[test]
fn captured_lights_use_modelview_at_replay_barrier() {
    let (mut context, _) = scene();
    context.current_tid = Some(3);
    context.modelview_matrix[12] = 5.0;
    let mut process = XpProcess::new_child();
    process.gl_runtime = Some(GlRuntime {
        device: unsafe { core::mem::zeroed() },
        queue: unsafe { core::mem::zeroed() },
        contexts: HashMap::from([(1, context)]),
        next_context: 2,
        triangle_renderer: None,
        textured_renderer: None,
        fixed_renderer: None,
        #[cfg(feature = "gpu-raster")]
        prepared_renderer: None,
    });
    let position = [1.0f32.to_bits(), 2.0f32.to_bits(), 3.0f32.to_bits(), 1.0f32.to_bits()];
    process.replay_gl_lightfv(3, FIXED_GL_LIGHT0, GL_POSITION, position).unwrap();
    let mut modelview = GL_IDENTITY_MATRIX;
    modelview[12] = 10.0;
    process.replay_gl_matrix_mode(3, GL_MODELVIEW).unwrap();
    process.replay_gl_load_matrixf(3, modelview.map(f32::to_bits)).unwrap();
    process.replay_gl_lightfv(3, FIXED_GL_LIGHT0 + 1, GL_POSITION, position).unwrap();
    let mut projection = GL_IDENTITY_MATRIX.map(f32::to_bits);
    projection[0] = 2.0f32.to_bits();
    process.replay_gl_matrix_mode(3, GL_PROJECTION).unwrap();
    process.replay_gl_load_matrixf(3, projection).unwrap();
    let mut texture = GL_IDENTITY_MATRIX.map(f32::to_bits);
    // Loading matrices is a bit-preserving copy, including signed zero and NaN.
    texture[1] = 0x80000000;
    texture[2] = 0x7fc01234;
    process.replay_gl_matrix_mode(3, GL_TEXTURE).unwrap();
    process.replay_gl_load_matrixf(3, texture).unwrap();
    assert!(process.replay_gl_matrix_mode(3, 0xffff).is_err());
    assert!(process.replay_gl_matrix_mode(99, GL_MODELVIEW).is_err());
    assert!(process.replay_gl_load_matrixf(99, projection).is_err());
    let context = process.gl_context_mut(3, "test").unwrap();
    assert_eq!(context.fixed.lights[0].position_eye, [6.0, 2.0, 3.0, 1.0]);
    assert_eq!(context.fixed.lights[1].position_eye, [11.0, 2.0, 3.0, 1.0]);
    assert_eq!(context.modelview_matrix, modelview);
    assert_eq!(context.projection_matrix.map(f32::to_bits), projection);
    assert_eq!(context.texture_matrix.map(f32::to_bits), texture);
    assert_eq!(context.matrix_mode, GL_TEXTURE);
}

#[test]
#[cfg(not(feature = "preview-first-draw"))]
fn production_clear_draw_and_readback_use_the_cpu_frame() {
    let (mut context, mut memory) = scene();
    context.current_tid = Some(3);
    context.clear_color = [0.125, 0.25, 0.5, 1.0];
    // A leftover GPU isolation selection must not remove the restored scene.
    context.debug_isolate_texture = Some(999);
    let mut process = XpProcess::new_child();
    process.gl_runtime = Some(GlRuntime {
        // These types contain only integer handles. Zero is a valid Rust value,
        // but cannot name a GPU object; no GPU call is allowed before swap.
        device: unsafe { core::mem::zeroed() },
        queue: unsafe { core::mem::zeroed() },
        contexts: HashMap::from([(1, context)]),
        next_context: 2,
        triangle_renderer: None,
        textured_renderer: None,
        fixed_renderer: None,
        #[cfg(feature = "gpu-raster")]
        prepared_renderer: None,
    });
    memory.0.resize(1600, 0);
    let write_call = |memory: &mut ArrayMemory, words: &[u32]| {
        for (i, word) in words.iter().enumerate() {
            memory.0[256 + i * 4..260 + i * 4].copy_from_slice(&word.to_le_bytes());
        }
    };
    write_call(&mut memory, &[0, GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT]);
    process.gl_clear_static(3, 256, &memory).unwrap();
    let clear_pixels = process.gl_context_mut(3, "test").unwrap()
        .raster_frame.as_ref().unwrap().rgba.clone();
    assert!(clear_pixels.chunks_exact(4).all(|p| p == [32, 64, 128, 255]));

    memory.0[128..131].copy_from_slice(&[0, 1, 2]);
    write_call(&mut memory, &[0, GL_TRIANGLES, 3, GL_UNSIGNED_BYTE, 128]);
    process.gl_draw_elements_static(3, 256, &memory).unwrap();
    let context = process.gl_context_mut(3, "test").unwrap();
    assert_eq!(context.draw_count, 1);
    let rendered = context.raster_frame.as_ref().unwrap().rgba.clone();
    assert_ne!(rendered, clear_pixels);

    write_call(&mut memory, &[0, 0, 0, 16, 16, 0x1908, GL_UNSIGNED_BYTE, 512]);
    process.gl_read_pixels_static(3, 256, &mut memory).unwrap();
    assert_eq!(&memory.0[512..1536], rendered.as_slice());
    let runtime = process.gl_runtime.as_ref().unwrap();
    assert!(runtime.textured_renderer.is_none());
    assert!(runtime.fixed_renderer.is_none());
}

#[test]
fn guest_arrays_lighting_fog_mips_depth_and_blend_reach_owned_frame() {
    let (mut c, mut memory) = scene();
    let (stats, unique) = gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).unwrap();
    assert_eq!(unique, 3);
    assert!(stats.shaded_pixels > 10);
    let original = pixel(&c, 8, 7);
    assert_eq!(original[0..2], [64, 32]);
    assert!((143..=144).contains(&original[2]));
    assert_eq!(original[3], 255);
    assert_eq!(pixel(&c, 0, 0), [0; 4]);

    // A later draw behind the scene must preserve accumulated color and depth.
    for i in 0..3 {
        memory.0[12 + i * 36..16 + i * 36].copy_from_slice(&(-3f32).to_le_bytes());
    }
    c.fixed.fog.color = [1., 0., 0., 1.];
    gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).unwrap();
    assert_eq!(pixel(&c, 8, 7), original);

    // Front geometry with failing alpha must not replace either buffer.
    for i in 0..3 {
        memory.0[12 + i * 36..16 + i * 36].copy_from_slice(&(-1.5f32).to_le_bytes());
    }
    c.fixed.set_enabled(0xb60, false).unwrap();
    c.fixed.set_enabled(0xbc0, true).unwrap();
    c.fixed.alpha_func = 0x204;
    c.fixed.alpha_ref = 0.75;
    for image in c.textures.object_mut().levels.values_mut() {
        image.internal = 0x1908;
        for p in image.rgba.chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 0, 128]);
        }
    }
    let depth_before = c.raster_frame.as_ref().unwrap().depth.clone();
    gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).unwrap();
    assert_eq!(pixel(&c, 8, 7), original);
    assert_eq!(c.raster_frame.as_ref().unwrap().depth, depth_before);

    c.fixed.set_enabled(0xbc0, false).unwrap();
    c.fixed.set_enabled(0xbe2, true).unwrap();
    c.fixed.blend_factors = [0x302, 0x303];
    gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).unwrap();
    let blended = pixel(&c, 8, 7);
    assert!((159..=160).contains(&blended[0]));
    assert_eq!(blended[1], 16);
    assert!((71..=72).contains(&blended[2]));
}

#[test]
fn bad_guest_vertex_or_incomplete_texture_cannot_partially_draw() {
    let (mut c, memory) = scene();
    gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).unwrap();
    let before = c.raster_frame.as_ref().unwrap().rgba.clone();
    assert!(gl_rasterize_elements(&mut c, &memory, &[0, 1, 2, 0, 1, 999]).is_err());
    assert_eq!(c.raster_frame.as_ref().unwrap().rgba, before);
    c.textures.object_mut().levels.remove(&1);
    assert!(gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).is_err());
    assert_eq!(c.raster_frame.as_ref().unwrap().rgba, before);
    c.textures.object_mut().levels.get_mut(&0).unwrap().width = 0;
    assert!(gl_rasterize_elements(&mut c, &memory, &[0, 1, 2]).is_err());
}

#[test]
fn disabled_lighting_uses_client_color_even_after_light_state_writes() {
    let (mut c, memory) = scene();
    c.fixed.set_enabled(0xb50, false).unwrap();
    c.fixed.light_model_two_side = true;
    c.color_array_enabled = true;
    c.color_pointer = Some(GlArrayPointer {
        size: 4,
        kind: GL_UNSIGNED_BYTE,
        stride: 36,
        address: 28,
    });
    let (vertices, indices) = gl_compat_vertices(&c, &memory, &[0, 1, 2, 0, 2, 1]).unwrap();
    assert_eq!(vertices.len(), 3);
    assert_eq!(indices, [0, 1, 2, 0, 2, 1]);
    assert_eq!(vertices[0].color, [1.; 4]);
    assert_eq!(vertices[0].clip[3], 2.);
}

#[test]
fn strip_publication_preserves_rows_edges_and_partial_last_strip() {
    let (width, height) = (4u32, 513u32);
    let source: Vec<u8> = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, (y >> 8) as u8, 17]))
        .collect();
    let mut frame = raster::Frame::new(width, height).unwrap();
    let state = raster::RasterState {
        viewport: [0, 0, width as i32, height as i32],
        tex_env: raster::TexEnvMode::Replace,
        ..Default::default()
    };
    let mut written = 0;
    for top in (0..height).step_by(256) {
        let rows = (height - top).min(256);
        let pixels = gl_present_strip_pixels(&source, width, height, top, rows);
        assert_eq!(pixels.len(), width as usize * rows as usize * 4);
        let levels = [raster::TextureLevel {
            width,
            height: rows,
            rgba: &pixels,
        }];
        let texture = raster::TextureView {
            levels: &levels,
            format: raster::TextureFormat::Rgba,
            wrap_s: raster::Wrap::Repeat,
            wrap_t: raster::Wrap::Repeat,
            min_filter: raster::Filter::Nearest,
            mag_filter: raster::Filter::Nearest,
        };
        let vertices = gl_present_strip_vertices(height, top, rows).map(|v| raster::ClipVertex {
            clip: [v.position[0], v.position[1], 0., 1.],
            color: [1.; 4],
            uv: [v.uv[0], v.uv[1], 0., 1.],
            fog: 0.,
        });
        written += frame
            .draw_triangles(&vertices, &[0, 1, 2, 0, 2, 3], &state, Some(&texture))
            .unwrap()
            .shaded_pixels;
    }
    assert_eq!(written, u64::from(width) * u64::from(height));
    for (expected, actual) in source.chunks_exact(4).zip(frame.rgba.chunks_exact(4)) {
        assert_eq!(&expected[..3], &actual[..3]);
        assert_eq!(expected[3], 17); // Presentation must not modify GL alpha.
        assert_eq!(actual[3], 255);
    }
}

#[test]
fn fullscreen_publication_budget_includes_resident_sampler_copy() {
    let page = |n: u64| (n + 4095) & !4095;
    let surface = page(2560 * 1440 * 4);
    let full_upload = page(2560 * 1440 * 4);
    let geometry = page(80) + page(24);
    let quota = 32 * 1024 * 1024;
    assert!(surface + 2 * full_upload + geometry > quota);
    let strip = page(2560 * 256 * 4);
    assert!(surface + 2 * strip + geometry < quota);
}

#[test]
fn triangle_strip_quad_preserves_both_faces_with_backface_culling() {
    let indices = gl_assemble_triangles(GL_TRIANGLE_STRIP, vec![0, 1, 2, 3]).unwrap();
    assert_eq!(indices, [0, 1, 2, 2, 1, 3]);
    let vertices = [[-1., -1.], [1., -1.], [-1., 1.], [1., 1.]].map(|p| raster::ClipVertex {
        clip: [p[0], p[1], 0., 1.],
        color: [1.; 4],
        uv: [0., 0., 0., 1.],
        fog: 0.,
    });
    let mut frame = raster::Frame::new(8, 8).unwrap();
    let state = raster::RasterState {
        viewport: [0, 0, 8, 8],
        cull: raster::Cull::Back,
        ..Default::default()
    };
    let stats = frame
        .draw_triangles(&vertices, &indices, &state, None)
        .unwrap();
    assert_eq!(stats.shaded_pixels, 64);
    assert!(frame.rgba.iter().all(|b| *b == 255));
}

#[test]
fn strip_degenerates_keep_parity_and_short_primitives_are_empty() {
    assert_eq!(
        gl_assemble_triangles(GL_TRIANGLE_STRIP, vec![0, 1, 2, 2, 3, 4]).unwrap(),
        [0, 1, 2, 2, 1, 2, 2, 2, 3, 3, 2, 4]
    );
    for mode in [GL_TRIANGLES, GL_TRIANGLE_STRIP] {
        for count in 0..3 {
            assert!(
                gl_assemble_triangles(mode, vec![9; count])
                    .unwrap()
                    .is_empty()
            );
        }
    }
    assert_eq!(
        gl_assemble_triangles(GL_TRIANGLES, vec![0, 1, 2, 3, 4]).unwrap(),
        [0, 1, 2]
    );
    assert!(gl_assemble_triangles(1, vec![0, 1, 2]).is_err());
}

#[test]
fn glyph_foreground_replaces_shadow_after_reusing_guest_color_buffer() {
    let mut c = WglContext::new(1, 2, 1);
    c.drawable_size = [8, 8];
    c.viewport = [0, 0, 8, 8];
    c.vertex_array_enabled = true;
    c.color_array_enabled = true;
    c.vertex_pointer = Some(GlArrayPointer { size: 3, kind: GL_FLOAT, stride: 12, address: 4 });
    c.color_pointer = Some(GlArrayPointer { size: 4, kind: GL_UNSIGNED_BYTE, stride: 0, address: 52 });
    let mut memory = ArrayMemory(vec![0; 68]);
    for (i, value) in [-1.0f32, -1., 0., 1., -1., 0., 1., 1., 0., -1., 1., 0.].into_iter().enumerate() {
        memory.0[4 + i * 4..8 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    c.textures.enabled = true;
    c.textures.bind(39).unwrap();
    c.textures.set_image(0, GlTextureImage { width: 1, height: 1, internal: 0x1908, rgba: vec![255, 255, 255, 255] }).unwrap();
    c.textures.object_mut().min_filter = 0x2601;
    c.fixed.set_enabled(0xbe2, true).unwrap();
    c.fixed.blend_factors = [0x302, 0x303];
    c.fixed.set_enabled(0xbc0, true).unwrap();
    c.fixed.alpha_func = 0x206;
    c.fixed.alpha_ref = 0.0;
    let indices = [0, 1, 2, 0, 2, 3];
    for p in memory.0[52..68].chunks_exact_mut(4) { p.copy_from_slice(&[0, 0, 0, 255]); }
    let shadow = gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0;
    assert_eq!(shadow.shaded_pixels, 64);
    // XPAPP binds its scratch color array before populating it, and reuses that
    // allocation for shadow and foreground. Read colors at each actual draw.
    for p in memory.0[52..68].chunks_exact_mut(4) { p.copy_from_slice(&[252, 210, 17, 255]); }
    let foreground = gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0;
    assert_eq!(foreground.shaded_pixels, 64);
    assert!(c.raster_frame.as_ref().unwrap().rgba.chunks_exact(4).all(|p| p == [252, 210, 17, 255]));
    // Model the suspected failure: existing depth rejects both glyph passes.
    c.fixed.set_enabled(0xb71, true).unwrap();
    c.fixed.depth_func = 0x201;
    c.raster_frame.as_mut().unwrap().depth.fill(0.0);
    c.raster_frame.as_mut().unwrap().rgba.fill(0);
    assert_eq!(gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0.shaded_pixels, 0);
    c.debug_depth_texture = Some(39);
    assert_eq!(gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0.shaded_pixels, 64);
    assert!(c.raster_frame.as_ref().unwrap().rgba.chunks_exact(4).all(|p| p == [252, 210, 17, 255]));
    assert!(c.raster_frame.as_ref().unwrap().depth.iter().all(|&z| z == 0.0));
    assert!(c.fixed.is_enabled(0xb71));
    // A different texture is unaffected, and restore reinstates rejection.
    c.debug_depth_texture = Some(40);
    assert_eq!(gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0.shaded_pixels, 0);
    c.debug_depth_texture = None;
    assert_eq!(gl_rasterize_elements(&mut c, &memory, &indices).unwrap().0.shaded_pixels, 0);

}

#[test]
fn guest_720p_drawable_replaces_1440p_cpu_storage() {
    let (mut c, _) = scene();
    c.drawable_size = [2560, 1440];
    XpProcess::gl_ensure_raster(&mut c).unwrap();
    assert_eq!(c.raster_frame.as_ref().unwrap().rgba.len(), 2560 * 1440 * 4);
    // This is the input bind_gl_ui4_window receives from the guest WindowObject.
    c.drawable_size = [1280, 720];
    c.viewport = [0, 0, 1280, 720];
    XpProcess::gl_ensure_raster(&mut c).unwrap();
    let frame = c.raster_frame.as_ref().unwrap();
    assert_eq!((frame.width, frame.height), (1280, 720));
    assert_eq!(frame.rgba.len(), 1280 * 720 * 4);
    assert_eq!(frame.depth.len(), 1280 * 720);
}

#[test]
fn draw_scratch_reuses_capacity_but_refreshes_guest_data_and_remaps() {
    let (c, source) = scene();
    let mut memory = CountingArrayMemory {
        memory: source, reads: core::cell::Cell::new(0), reject_bulk: false,
    };
    let mut scratch = GlDrawScratch::default();
    let mut timing = crate::frame_heartbeat::DecodeTiming::default();
    let (vertices, indices) = gl_compat_vertices_reusing(&c, &memory, &[0, 1, 2, 0, 2, 1], &mut timing, &mut scratch).unwrap();
    let old_clip = vertices[0].clip;
    let vertex_ptr = vertices.as_ptr();
    let index_ptr = indices.as_ptr();
    let remap_ptr = scratch.dense_remap.as_ptr();
    let snapshot_ptrs: Vec<_> = scratch.snapshot_ranges.iter().map(|(_, b)| b.as_ptr()).collect();
    scratch.vertices = vertices;
    scratch.indices = indices;
    memory.memory.0[4..8].copy_from_slice(&0.25f32.to_le_bytes());
    let (vertices, indices) = gl_compat_vertices_reusing(&c, &memory, &[2, 0, 1], &mut timing, &mut scratch).unwrap();
    assert_eq!(vertices.as_ptr(), vertex_ptr);
    assert_eq!(indices.as_ptr(), index_ptr);
    assert_eq!(scratch.dense_remap.as_ptr(), remap_ptr);
    assert_eq!(scratch.snapshot_ranges.iter().map(|(_, b)| b.as_ptr()).collect::<Vec<_>>(), snapshot_ptrs);
    assert_eq!(indices, [0, 1, 2]);
    assert_ne!(vertices[1].clip, old_clip);
    scratch.vertices = vertices;
    scratch.indices = indices;
    // Failed bulk reads may have written partial bytes; the fallback must read
    // fresh individual attributes, including when older storage exists.
    memory.reject_bulk = true;
    memory.memory.0[4..8].copy_from_slice(&0.75f32.to_le_bytes());
    let (actual, mapped) = gl_compat_vertices_reusing(&c, &memory, &[0, 1, 2], &mut timing, &mut scratch).unwrap();
    let (expected, expected_mapped) = gl_compat_vertices(&c, &memory, &[0, 1, 2]).unwrap();
    assert_eq!(mapped, expected_mapped);
    for (a, b) in actual.iter().zip(expected) {
        assert_eq!(a.clip, b.clip);
        assert_eq!(a.color, b.color);
        assert_eq!(a.uv, b.uv);
    }
}

#[test]
fn reusable_triangle_assembly_matches_existing_topology() {
    let mut output = Vec::new();
    for mode in [GL_TRIANGLES, GL_TRIANGLE_STRIP] {
        for indices in [vec![0, 1, 1, 2, 3, 4, 5], vec![9, 7, 6], vec![], vec![1, 2]] {
            gl_assemble_triangles_into(mode, &indices, &mut output).unwrap();
            assert_eq!(output, gl_assemble_triangles(mode, indices).unwrap());
        }
    }
}
