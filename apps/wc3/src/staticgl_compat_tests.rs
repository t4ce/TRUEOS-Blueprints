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
