// Runtime bridge: state packing and guest-array decoding only. The GPU performs
// transforms, lighting, clipping, interpolation, sampling, fog and depth testing.
struct GlFixedGpuDraw {
    vertices: Vec<[f32; 16]>,
    indices: Vec<u32>,
    state: [f32; 384],
}

// Intel's color-buffer blend factors use the same compact numbering as Mesa's
// PIPE_BLENDFACTOR values. Keep that hardware-independent ABI in the packed
// fixed state so the kernel can validate it before programming the renderer.
fn gl_fixed_gpu_blend_factor(value: u32) -> Result<f32, ProviderDispatchError> {
    let encoded = match value {
        FIXED_GL_ONE => 0x01,
        FIXED_GL_SRC_COLOR => 0x02,
        FIXED_GL_SRC_ALPHA => 0x03,
        FIXED_GL_DST_ALPHA => 0x04,
        FIXED_GL_DST_COLOR => 0x05,
        FIXED_GL_SRC_ALPHA_SATURATE => 0x06,
        FIXED_GL_CONSTANT_COLOR => 0x07,
        FIXED_GL_CONSTANT_ALPHA => 0x08,
        FIXED_GL_ZERO => 0x11,
        FIXED_GL_ONE_MINUS_SRC_COLOR => 0x12,
        FIXED_GL_ONE_MINUS_SRC_ALPHA => 0x13,
        FIXED_GL_ONE_MINUS_DST_ALPHA => 0x14,
        FIXED_GL_ONE_MINUS_DST_COLOR => 0x15,
        FIXED_GL_ONE_MINUS_CONSTANT_COLOR => 0x17,
        FIXED_GL_ONE_MINUS_CONSTANT_ALPHA => 0x18,
        _ => return Err(gl_texture_error("glDrawElements", "fixed GPU blend factor")),
    };
    Ok(encoded as f32)
}

fn gl_fixed_gpu_alpha_blend_factor(value: u32) -> Result<f32, ProviderDispatchError> {
    gl_fixed_gpu_blend_factor(match value {
        FIXED_GL_SRC_COLOR => FIXED_GL_SRC_ALPHA,
        FIXED_GL_ONE_MINUS_SRC_COLOR => FIXED_GL_ONE_MINUS_SRC_ALPHA,
        FIXED_GL_DST_COLOR => FIXED_GL_DST_ALPHA,
        FIXED_GL_ONE_MINUS_DST_COLOR => FIXED_GL_ONE_MINUS_DST_ALPHA,
        FIXED_GL_CONSTANT_COLOR => FIXED_GL_CONSTANT_ALPHA,
        FIXED_GL_ONE_MINUS_CONSTANT_COLOR => FIXED_GL_ONE_MINUS_CONSTANT_ALPHA,
        FIXED_GL_SRC_ALPHA_SATURATE => FIXED_GL_ONE,
        other => other,
    })
}

fn gl_fixed_gpu_wrap(value: u32) -> Result<f32, ProviderDispatchError> {
    match value {
        0x2901 => Ok(0.), // REPEAT
        0x2900 => Ok(1.), // legacy CLAMP with border color
        0x812f => Ok(2.), // CLAMP_TO_EDGE
        _ => Err(gl_texture_error("glDrawElements", "fixed GPU texture wrap")),
    }
}

fn gl_fixed_gpu_state(c: &WglContext) -> Result<Option<[f32; 384]>, ProviderDispatchError> {
    const API: &str = "glDrawElements";
    let f = &c.fixed;
    let allowed = [
        FIXED_GL_LIGHTING,
        FIXED_GL_FOG,
        FIXED_GL_DEPTH_TEST,
        FIXED_GL_CULL_FACE,
        FIXED_GL_SCISSOR_TEST,
        FIXED_GL_COLOR_MATERIAL,
        FIXED_GL_NORMALIZE,
        FIXED_GL_TEXTURE_2D,
        FIXED_GL_DITHER,
        FIXED_GL_ALPHA_TEST,
        FIXED_GL_BLEND,
    ]
    .into_iter()
    .chain(FIXED_GL_LIGHT0..=FIXED_GL_LIGHT7)
    .fold(0u64, |m, cap| {
        m | 1u64 << GlFixedState::capability_slot(cap).unwrap()
    });
    if f.enabled & !allowed != 0 {
        return Err(gl_texture_error(
            API,
            format!(
                "fixed GPU unsupported capabilities=0x{:x}",
                f.enabled & !allowed
            ),
        ));
    }
    if f.light_model_two_side {
        return Err(gl_texture_error(
            API,
            "fixed GPU two-sided lighting pending",
        ));
    }
    let [width, height] = c.drawable_size;
    if width == 0 || height == 0 {
        return Ok(None);
    }
    if c.viewport != [0, 0, width as i32, height as i32] || f.depth_range != [0., 1.] {
        return Err(gl_texture_error(
            API,
            "fixed GPU nondefault viewport/depth range pending",
        ));
    }
    let [x, y, w, h] = if f.is_enabled(FIXED_GL_SCISSOR_TEST) {
        f.scissor.map(i64::from)
    } else {
        [0, 0, i64::from(width), i64::from(height)]
    };
    let left = x.clamp(0, width as i64);
    let right = (x + w).clamp(0, width as i64);
    let bottom = y.clamp(0, height as i64);
    let top = (y + h).clamp(0, height as i64);
    if w <= 0 || h <= 0 || left >= right || bottom >= top {
        return Ok(None);
    }
    let mut s = [[0f32; 4]; 96];
    for i in 0..4 {
        s[i].copy_from_slice(&c.modelview_matrix[i * 4..i * 4 + 4]);
        s[4 + i].copy_from_slice(&c.projection_matrix[i * 4..i * 4 + 4]);
        s[8 + i].copy_from_slice(&c.texture_matrix[i * 4..i * 4 + 4]);
    }
    if f.is_enabled(FIXED_GL_LIGHTING) {
        let normal = gl_normal_matrix(&c.modelview_matrix)?;
        for i in 0..3 {
            s[12 + i][..3].copy_from_slice(&normal[i]);
        }
    }
    s[15][3] = 1.;
    s[16] = f.light_model_ambient;
    let m = &f.materials[0];
    s[17] = m.ambient;
    s[18] = m.diffuse;
    s[19] = m.specular;
    s[20] = m.emission;
    s[21] = [
        m.shininess,
        f.is_enabled(FIXED_GL_LIGHTING) as u8 as f32,
        f.is_enabled(FIXED_GL_NORMALIZE) as u8 as f32,
        f.light_model_local_viewer as u8 as f32,
    ];
    if f.is_enabled(FIXED_GL_COLOR_MATERIAL)
        && matches!(
            f.color_material_face,
            FIXED_GL_FRONT | FIXED_GL_FRONT_AND_BACK
        )
    {
        s[22][0] = match f.color_material_mode {
            0x1200 => 1.,
            0x1201 => 2.,
            0x1202 => 3.,
            FIXED_GL_EMISSION => 4.,
            FIXED_GL_AMBIENT_AND_DIFFUSE => 5.,
            _ => return Err(gl_texture_error(API, "fixed GPU color material mode")),
        };
    }
    s[23] = [
        if f.is_enabled(FIXED_GL_FOG) {
            match f.fog.mode {
                FIXED_GL_LINEAR => 1.,
                FIXED_GL_EXP => 2.,
                FIXED_GL_EXP2 => 3.,
                _ => return Err(gl_texture_error(API, "fixed GPU fog mode")),
            }
        } else {
            0.
        },
        f.fog.density,
        f.fog.start,
        f.fog.end,
    ];
    if s[23][0] == 1. && f.fog.start == f.fog.end {
        return Err(gl_texture_error(API, "undefined linear fog interval"));
    }
    s[24] = f.fog.color;
    if c.textures.enabled {
        let image = gl_texture_draw_image_contract(&c.textures, true)?;
        let object = c.textures.object();
        let mode = match object.min_filter {
            0x2600 => 0,
            0x2601 => 1,
            n => n - 0x2700 + 2,
        };
        let levels = gl_fixed_gpu_mip_count(&c.textures)?;
        s[30] = [
            image.width as f32,
            image.height as f32,
            mode as f32,
            (object.mag_filter == 0x2601) as u8 as f32,
        ];
        s[31][0] = (levels - 1) as f32;

        s[25] = [
            match c.textures.env_mode {
                0x2100 => 1.,
                0x2101 => 2.,
                0x1e01 => 3.,
                0xbe2 => 4.,
                _ => return Err(gl_texture_error(API, "fixed GPU texture environment")),
            },
            (image.internal == 0x1907) as u8 as f32,
            gl_fixed_gpu_wrap(object.wrap_s)?,
            gl_fixed_gpu_wrap(object.wrap_t)?,
        ];
    }
    s[31][1] = f.is_enabled(FIXED_GL_ALPHA_TEST) as u8 as f32;
    s[31][2] = (f.alpha_func - FIXED_GL_NEVER) as f32;
    s[31][3] = f.alpha_ref;
    s[88] = [
        f.is_enabled(FIXED_GL_BLEND) as u8 as f32,
        gl_fixed_gpu_blend_factor(f.blend_factors[0])?,
        gl_fixed_gpu_blend_factor(f.blend_factors[1])?,
        0.,
    ];
    s[89] = [
        gl_fixed_gpu_alpha_blend_factor(f.blend_factors[0])?,
        gl_fixed_gpu_alpha_blend_factor(f.blend_factors[1])?,
        0.,
        0.,
    ];
    s[27] = [1., 1., 0., 0.];
    s[28] = [0., 1., f.is_enabled(FIXED_GL_CULL_FACE) as u8 as f32, 0.];
    s[29] = [
        left as f32,
        (height as i64 - top) as f32,
        right as f32,
        (height as i64 - bottom) as f32,
    ];
    for (i, l) in f.lights.iter().enumerate() {
        let b = 32 + i * 7;
        s[b] = l.position_eye;
        s[b + 1] = l.ambient;
        s[b + 2] = l.diffuse;
        s[b + 3] = l.specular;
        s[b + 4] = [
            l.spot_direction_eye[0],
            l.spot_direction_eye[1],
            l.spot_direction_eye[2],
            l.spot_cutoff.to_radians().cos(),
        ];
        s[b + 5] = [
            l.attenuation[0],
            l.attenuation[1],
            l.attenuation[2],
            l.spot_exponent,
        ];
        s[b + 6] = [
            f.is_enabled(FIXED_GL_LIGHT0 + i as u32) as u8 as f32,
            (l.spot_cutoff == 180.) as u8 as f32,
            0.,
            0.,
        ];
    }
    let state = core::array::from_fn(|i| s[i / 4][i % 4]);
    if state.iter().any(|v| !v.is_finite()) {
        return Err(gl_texture_error(API, "nonfinite fixed GPU state"));
    }
    Ok(Some(state))
}
fn gl_fixed_gpu_geometry(
    c: &WglContext,
    memory: &impl GuestMemory,
    indices: &[u32],
) -> Result<GlFixedGpuDraw, ProviderDispatchError> {
    let state = gl_fixed_gpu_state(c)?;
    let mut result = GlFixedGpuDraw {
        vertices: Vec::new(),
        indices: Vec::new(),
        state: state.unwrap_or([0.; 384]),
    };
    if state.is_none() {
        return Ok(result);
    }
    if !c.vertex_array_enabled {
        return Err(gl_texture_error("glDrawElements", "vertex array disabled"));
    }
    let memory = GlArraySnapshot::new(memory, c, indices);
    let mut remap = GlIndexRemap::new(indices);
    for &index in indices {
        if let Some(mapped) = remap.get(&index) {
            result.indices.push(mapped);
            continue;
        }
        let position = gl_read_array(
            &memory,
            c.vertex_pointer.ok_or("missing vertex pointer")?,
            index,
            [0., 0., 0., 1.],
        )?;
        let normal = if c.fixed.is_enabled(FIXED_GL_LIGHTING) && c.fixed.normal_array_enabled {
            gl_read_array(
                &memory,
                c.fixed.normal_pointer.ok_or("missing normal pointer")?,
                index,
                [0.; 4],
            )?
        } else {
            [
                c.fixed.current_normal[0],
                c.fixed.current_normal[1],
                c.fixed.current_normal[2],
                0.,
            ]
        };
        let color = if c.color_array_enabled {
            gl_read_array(
                &memory,
                c.color_pointer.ok_or("missing color pointer")?,
                index,
                [1.; 4],
            )?
        } else {
            [1.; 4]
        };
        let uv = if c.textures.enabled && c.textures.coord_array_enabled {
            gl_read_array(
                &memory,
                c.textures.coord_pointer.ok_or("missing texture pointer")?,
                index,
                [0., 0., 0., 1.],
            )?
        } else {
            [0., 0., 0., 1.]
        };
        let vertex = core::array::from_fn(|i| [position, normal, color, uv][i / 4][i % 4]);
        let mapped = result.vertices.len() as u32;
        result.vertices.push(vertex);
        result.indices.push(mapped);
        remap.insert(index, mapped);
    }
    Ok(result)
}

#[cfg(test)]
mod fixed_gpu_tests {
    use super::*;
    #[test]
    fn reported_wc3_mask_packs_all_lights_fog_and_lower_left_scissor() {
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.fixed.enabled = 0x300f04f;
        c.fixed.scissor = [10, 20, 100, 80];
        c.fixed.fog.mode = FIXED_GL_EXP2;
        c.fixed.fog.density = 0.125;
        c.fixed.lights[3].diffuse = [0.1, 0.2, 0.3, 1.];
        let s = gl_fixed_gpu_state(&c).unwrap().unwrap();
        assert_eq!(s[21 * 4 + 1], 1.);
        assert_eq!(&s[23 * 4..24 * 4], &[3., 0.125, 0., 1.]);
        assert_eq!(&s[29 * 4..30 * 4], &[10., 380., 110., 460.]);
        for i in 0..8 {
            assert_eq!(s[(32 + i * 7 + 6) * 4], if i < 4 { 1. } else { 0. });
        }
        assert_eq!(
            &s[(32 + 3 * 7 + 2) * 4..(32 + 3 * 7 + 3) * 4],
            &[0.1, 0.2, 0.3, 1.]
        );
        assert!(c.raster_frame.is_none());
    }
    #[test]
    fn material_modes_normal_matrix_and_transforms_keep_their_slots() {
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.fixed.set_enabled(FIXED_GL_LIGHTING, true).unwrap();
        c.fixed.set_enabled(FIXED_GL_COLOR_MATERIAL, true).unwrap();
        c.modelview_matrix[0] = 2.;
        c.modelview_matrix[5] = 4.;
        c.modelview_matrix[10] = 8.;
        c.projection_matrix[11] = -1.;
        c.projection_matrix[15] = 0.;
        for (mode, value) in [
            (0x1200, 1.),
            (0x1201, 2.),
            (0x1202, 3.),
            (0x1600, 4.),
            (0x1602, 5.),
        ] {
            c.fixed.color_material_mode = mode;
            let s = gl_fixed_gpu_state(&c).unwrap().unwrap();
            assert_eq!(s[22 * 4], value);
            assert_eq!(&s[..16], &c.modelview_matrix);
            assert_eq!(&s[16..32], &c.projection_matrix);
            assert_eq!(
                [s[12 * 4], s[13 * 4 + 1], s[14 * 4 + 2]],
                [0.5, 0.25, 0.125]
            );
        }
    }
    #[test]
    fn guest_vertices_reach_gpu_without_projection_lighting_or_cpu_culling() {
        struct Memory(Vec<u8>);
        impl GuestMemory for Memory {
            fn read(&self, a: u32, out: &mut [u8]) -> Result<(), &'static str> {
                out.copy_from_slice(
                    self.0
                        .get(a as usize..a as usize + out.len())
                        .ok_or("bounds")?,
                );
                Ok(())
            }
            fn write(&mut self, _: u32, _: &[u8]) -> Result<(), &'static str> {
                panic!("no guest writes")
            }
        }
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.vertex_array_enabled = true;
        c.vertex_pointer = Some(GlArrayPointer {
            size: 4,
            kind: GL_FLOAT,
            stride: 16,
            address: 4,
        });
        c.fixed.set_enabled(FIXED_GL_LIGHTING, true).unwrap();
        c.projection_matrix[15] = 0.;
        c.projection_matrix[11] = -1.;
        let v = [2f32, 3., -4., 1., -2., 3., -4., 1., 0., -3., -4., 1.];
        let mut memory = vec![0; 4];
        for n in v {
            memory.extend_from_slice(&n.to_le_bytes());
        }
        let draw = gl_fixed_gpu_geometry(&c, &Memory(memory), &[0, 1, 2, 2, 1, 0]).unwrap();
        assert_eq!(draw.vertices.len(), 3);
        assert_eq!(draw.indices, [0, 1, 2, 2, 1, 0]);
        assert_eq!(&draw.vertices[0][..4], &v[..4]);
        assert_eq!(&draw.vertices[0][8..12], &[1.; 4]);
        assert!(c.raster_frame.is_none());
    }
    #[test]
    fn empty_scissor_skips_and_alpha_blend_state_is_packed() {
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.fixed.set_enabled(FIXED_GL_SCISSOR_TEST, true).unwrap();
        c.fixed.scissor = [640, 0, i32::MAX, 480];
        assert!(gl_fixed_gpu_state(&c).unwrap().is_none());
        c.fixed.set_enabled(FIXED_GL_SCISSOR_TEST, false).unwrap();
        c.fixed.set_enabled(FIXED_GL_BLEND, true).unwrap();
        c.fixed
            .set_blend_func(FIXED_GL_SRC_ALPHA, FIXED_GL_ONE_MINUS_SRC_ALPHA)
            .unwrap();
        c.fixed.set_enabled(FIXED_GL_ALPHA_TEST, true).unwrap();
        c.fixed.set_alpha_func(FIXED_GL_GREATER, 0.25).unwrap();
        let s = gl_fixed_gpu_state(&c).unwrap().unwrap();
        assert_eq!(&s[125..128], &[1., 4., 0.25]);
        assert_eq!(&s[352..358], &[1., 3., 19., 0., 3., 19.]);
        c.textures.enabled = true;
        c.textures.bind(1).unwrap();
        c.textures.object_mut().wrap_s = 0x2901;
        c.textures.object_mut().wrap_t = 0x2900;
        c.textures.object_mut().levels.insert(0, GlTextureImage {
            width: 1,
            height: 1,
            internal: 0x1908,
            rgba: vec![255; 4],
        });
        let s = gl_fixed_gpu_state(&c).unwrap().unwrap();
        assert_eq!(&s[102..104], &[0., 1.]);
    }
}

// Pack authored levels into vertically stacked rows at the base-level pitch.
// The GPU handles sampling; this copies texture bytes without resampling them.
fn gl_fixed_gpu_mip_count(textures: &GlTextures) -> Result<u32, ProviderDispatchError> {
    let image = gl_texture_draw_image_contract(textures, true)?;
    let object = textures.object();
    let count = if object.min_filter >= 0x2700 {
        32 - image.width.max(image.height).leading_zeros()
    } else {
        1
    };
    for level in 0..count {
        let w = (image.width >> level).max(1);
        let h = (image.height >> level).max(1);
        let mip = object.levels.get(&level).ok_or_else(|| {
            gl_texture_error(
                "glDrawElements",
                format!("incomplete texture mip chain: missing level {level}"),
            )
        })?;
        if mip.width != w
            || mip.height != h
            || mip.internal != image.internal
            || (w as usize)
                .checked_mul(h as usize)
                .and_then(|n| n.checked_mul(4))
                != Some(mip.rgba.len())
        {
            return Err(gl_texture_error(
                "glDrawElements",
                format!("incomplete texture mip chain: invalid level {level}"),
            ));
        }
    }
    Ok(count)
}
fn gl_fixed_gpu_texture(
    textures: &GlTextures,
) -> Result<(std::sync::Arc<[u8]>, u32, u32), ProviderDispatchError> {
    if let Some(atlas) = textures.fixed_atlases.borrow().get(&textures.binding) {
        return Ok(atlas.clone());
    }
    let levels = gl_fixed_gpu_mip_count(textures)?;
    let object = textures.object();
    let base = &object.levels[&0];
    let height = (0..levels).map(|i| (base.height >> i).max(1)).sum::<u32>();
    let pitch = (base.width as usize)
        .checked_mul(4)
        .ok_or("texture atlas pitch overflow")?;
    let len = pitch
        .checked_mul(height as usize)
        .ok_or("texture atlas size overflow")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| "texture atlas allocation failed")?;
    bytes.resize(len, 0);
    let mut row = 0;
    for level in 0..levels {
        let image = &object.levels[&level];
        let row_bytes = image.width as usize * 4;
        for source in image.rgba.chunks_exact(row_bytes) {
            bytes[row * pitch..row * pitch + row_bytes].copy_from_slice(source);
            row += 1;
        }
    }
    let atlas = (std::sync::Arc::<[u8]>::from(bytes), base.width, height);
    const CACHE_BYTES: usize = 16 * 1024 * 1024;
    if atlas.0.len() <= CACHE_BYTES {
        let mut cache = textures.fixed_atlases.borrow_mut();
        let used: usize = cache.values().map(|entry| entry.0.len()).sum();
        if cache.len() >= 64 || used + atlas.0.len() > CACHE_BYTES { cache.clear(); }
        cache.insert(textures.binding, atlas.clone());
    }
    Ok(atlas)
}

#[cfg(test)]
mod fixed_gpu_mip_tests {
    use super::*;
    fn texture() -> GlTextures {
        let mut t = GlTextures::default();
        t.bind(1).unwrap();
        let o = t.object_mut();
        o.min_filter = 0x2701;
        o.mag_filter = 0x2601;
        for (level, w, h, value) in [(0, 8, 2, 10), (1, 4, 1, 40), (2, 2, 1, 90), (3, 1, 1, 170)] {
            o.levels.insert(
                level,
                GlTextureImage {
                    width: w,
                    height: h,
                    internal: 0x1908,
                    rgba: vec![value; (w * h * 4) as usize],
                },
            );
        }
        t
    }
    #[test]
    fn cached_atlas_reuses_storage_and_invalidates_on_subimage_and_delete() {
        struct Pixel;
        impl GuestMemory for Pixel {
            fn read(&self, address: u32, out: &mut [u8]) -> Result<(), &'static str> {
                if address != 1 || out.len() != 4 { return Err("unexpected read"); }
                out.copy_from_slice(&[1, 2, 3, 255]); Ok(())
            }
            fn write(&mut self, _: u32, _: &[u8]) -> Result<(), &'static str> { Err("read only") }
        }
        let mut t = texture();
        let first = gl_fixed_gpu_texture(&t).unwrap().0;
        let repeated = gl_fixed_gpu_texture(&t).unwrap().0;
        assert!(std::sync::Arc::ptr_eq(&first, &repeated));
        t.sub_image(0, [0, 0, 1, 1], 0x1908, GL_UNSIGNED_BYTE, 1, &Pixel).unwrap();
        let changed = gl_fixed_gpu_texture(&t).unwrap().0;
        assert!(!std::sync::Arc::ptr_eq(&first, &changed));
        assert_eq!(&changed[..4], &[1, 2, 3, 255]);
        assert_eq!(&first[..4], &[10; 4]);
        t.delete(&[1]);
        t.bind(1).unwrap();
        assert!(gl_fixed_gpu_texture(&t).is_err());
    }

    #[test]
    fn reported_filters_keep_all_authored_levels_and_non_square_offsets() {
        let t = texture();
        let (atlas, w, h) = gl_fixed_gpu_texture(&t).unwrap();
        assert_eq!((w, h), (8, 5));
        for (level, row) in [(0, 0), (1, 2), (2, 3), (3, 4)] {
            let image = &t.object().levels[&level];
            for y in 0..image.height as usize {
                let bytes = image.width as usize * 4;
                assert_eq!(
                    &atlas[(row + y) * 32..(row + y) * 32 + bytes],
                    &image.rgba[y * bytes..(y + 1) * bytes]
                );
                assert!(
                    atlas[(row + y) * 32 + bytes..(row + y + 1) * 32]
                        .iter()
                        .all(|b| *b == 0)
                );
            }
        }
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [640, 480];
        c.viewport = [0, 0, 640, 480];
        c.textures = t;
        c.textures.enabled = true;
        let state = gl_fixed_gpu_state(&c).unwrap().unwrap();
        assert_eq!(&state[120..125], &[8., 2., 3., 1., 3.]);
    }
    #[test]
    fn every_minification_mode_and_both_magnification_modes_pack_distinctly() {
        let mut c = WglContext::new(1, 2, 1);
        c.drawable_size = [8, 8];
        c.viewport = [0, 0, 8, 8];
        c.textures = texture();
        c.textures.enabled = true;
        for (mode, filter) in [0x2600, 0x2601, 0x2700, 0x2701, 0x2702, 0x2703]
            .into_iter()
            .enumerate()
        {
            for mag in [0x2600, 0x2601] {
                c.textures.object_mut().min_filter = filter;
                c.textures.object_mut().mag_filter = mag;
                let s = gl_fixed_gpu_state(&c).unwrap().unwrap();
                assert_eq!(s[122], mode as f32);
                assert_eq!(s[123], (mag == 0x2601) as u8 as f32);
                assert_eq!(s[124], if mode < 2 { 0. } else { 3. });
            }
        }
    }
    #[test]
    fn incomplete_or_mismatched_mips_are_never_silently_replaced_by_level_zero() {
        let mut t = texture();
        t.object_mut().levels.remove(&2);
        assert!(gl_fixed_gpu_texture(&t).is_err());
        t.object_mut().min_filter = 0x2601;
        assert_eq!(gl_fixed_gpu_texture(&t).unwrap().2, 2);
        let mut t = texture();
        t.object_mut().levels.get_mut(&2).unwrap().width = 3;
        assert!(gl_fixed_gpu_texture(&t).is_err());
        let mut t = texture();
        t.object_mut().levels.get_mut(&2).unwrap().internal = 0x1907;
        assert!(gl_fixed_gpu_texture(&t).is_err());
        let mut t = texture();
        t.object_mut().levels.get_mut(&3).unwrap().rgba.clear();
        assert!(gl_fixed_gpu_texture(&t).is_err());
    }
}
