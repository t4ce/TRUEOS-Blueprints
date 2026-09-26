struct ArrayMemory(Vec<u8>);
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
