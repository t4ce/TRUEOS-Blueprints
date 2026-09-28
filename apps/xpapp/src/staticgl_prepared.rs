//! CPU-prepared geometry encoded for the frame-batched Warcraft raster shader.
use crate::staticgl_raster::*;
use staticgl_triangle::prepared::Draw;
use std::sync::Arc;
use trueos::vgpu::*;

fn blend(f: BlendFactor, alpha: bool) -> f32 {
    use BlendFactor::*;
    (match (f, alpha) {
        (Zero, _) => 0x11,
        (One, _) => 1,
        (SrcColor, false) => 2,
        (SrcColor, true) | (SrcAlpha, _) => 3,
        (OneMinusSrcColor, false) => 0x12,
        (OneMinusSrcColor, true) | (OneMinusSrcAlpha, _) => 0x13,
        (DstColor, false) => 5,
        (DstColor, true) | (DstAlpha, _) => 4,
        (OneMinusDstColor, false) => 0x15,
        (OneMinusDstColor, true) | (OneMinusDstAlpha, _) => 0x14,
        (SrcAlphaSaturate, false) => 6,
        (SrcAlphaSaturate, true) => 1,
    }) as f32
}
fn compare(f: Compare) -> u32 {
    match f {
        Compare::Never => 0,
        Compare::Less => 1,
        Compare::Equal => 2,
        Compare::Lequal => 3,
        Compare::Greater => 4,
        Compare::NotEqual => 5,
        Compare::Gequal => 6,
        Compare::Always => 7,
    }
}
fn wrap(w: Wrap) -> Result<f32, &'static str> {
    Ok(match w {
        Wrap::Repeat => 0.,
        Wrap::Clamp => 1.,
        Wrap::ClampToEdge => 2.,
        Wrap::MirroredRepeat => return Err("prepared mirrored repeat unsupported"),
    })
}
fn filter(f: Filter) -> f32 {
    match f {
        Filter::Nearest => 0.,
        Filter::Linear => 1.,
        Filter::NearestMipmapNearest => 2.,
        Filter::LinearMipmapNearest => 3.,
        Filter::NearestMipmapLinear => 4.,
        Filter::LinearMipmapLinear => 5.,
    }
}
fn scissor(extent: [u32; 2], s: Option<[i32; 4]>) -> Option<[f32; 4]> {
    let [width, height] = extent.map(i64::from);
    let [x, y, w, h] = s.map(|v| v.map(i64::from)).unwrap_or([0, 0, width, height]);
    let left = x.clamp(0, width);
    let right = (x + w).clamp(0, width);
    let bottom = y.clamp(0, height);
    let top = (y + h).clamp(0, height);
    (w > 0 && h > 0 && left < right && bottom < top).then_some([
        left as f32,
        (height - top) as f32,
        right as f32,
        (height - bottom) as f32,
    ])
}
fn base_state(extent: [u32; 2]) -> [f32; 384] {
    let mut s = [0.; 384];
    for matrix in 0..3 {
        for i in 0..4 {
            s[matrix * 16 + i * 5] = 1.;
        }
    }
    s[108..112].copy_from_slice(&[1., 1., 0., 0.]);
    s[112..116].copy_from_slice(&[0., 1., 0., 0.]);
    s[116..120].copy_from_slice(&[0., 0., extent[0] as f32, extent[1] as f32]);
    s[353..355].copy_from_slice(&[1.,17.]);
    s[356..358].copy_from_slice(&[1.,17.]);
    s[360] = 1.;
    s
}

pub(crate) fn draw(
    extent: [u32; 2],
    triangles: &[[ClipVertex; 3]],
    state: &RasterState,
    texture: Option<TextureView<'_>>,
    atlas: Arc<[u8]>,
    texture_size: [u32; 2],
) -> Result<Option<Draw>, &'static str> {
    if extent.contains(&0) {
        return Err("prepared drawable is empty");
    }
    let Some(rect) = scissor(extent, state.scissor_enabled.then_some(state.scissor)) else {
        return Ok(None);
    };
    let mut s = base_state(extent);
    let [vx, vy, vw, vh] = state.viewport.map(|v| v as f32);
    s[108..112].copy_from_slice(&[
        vw / extent[0] as f32,
        vh / extent[1] as f32,
        (2. * vx + vw) / extent[0] as f32 - 1.,
        (2. * vy + vh) / extent[1] as f32 - 1.,
    ]);
    s[116..120].copy_from_slice(&rect);
    s[92..96].copy_from_slice(&[
        if !state.fog.enabled {
            0.
        } else {
            match state.fog.mode {
                FogMode::Linear => 1.,
                FogMode::Exp => 2.,
                FogMode::Exp2 => 3.,
            }
        },
        state.fog.density,
        state.fog.start,
        state.fog.end,
    ]);
    s[96..100].copy_from_slice(&state.fog.color);
    s[104..108].copy_from_slice(&state.tex_env_color);
    if let Some(t) = texture {
        s[100] = match state.tex_env {
            TexEnvMode::Modulate => 1.,
            TexEnvMode::Decal => 2.,
            TexEnvMode::Replace => 3.,
            TexEnvMode::Blend => 4.,
        };
        s[101] = (t.format == TextureFormat::Rgb) as u8 as f32;
        s[102] = wrap(t.wrap_s)?;
        s[103] = wrap(t.wrap_t)?;
        s[120..124].copy_from_slice(&[
            t.levels[0].width as f32,
            t.levels[0].height as f32,
            filter(t.min_filter),
            (t.mag_filter == Filter::Linear) as u8 as f32,
        ]);
        s[124] = (t.levels.len() - 1) as f32;
        s[361] = match t.format {
            TextureFormat::Alpha => 0.,
            TextureFormat::Luminance => 1.,
            TextureFormat::LuminanceAlpha => 2.,
            TextureFormat::Rgb => 3.,
            TextureFormat::Rgba => 4.,
            TextureFormat::Intensity => 5.,
        };
    }
    s[125] = state.alpha.enabled as u8 as f32;
    s[126] = compare(state.alpha.func) as f32;
    s[127] = state.alpha.reference;
    s[352] = state.blend.enabled as u8 as f32;
    s[353] = blend(state.blend.src, false);
    s[354] = blend(state.blend.dst, false);
    s[356] = blend(state.blend.src, true);
    s[357] = blend(state.blend.dst, true);
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    for tri in triangles {
        let p = tri.map(|v| {
            [
                vx + (v.clip[0] / v.clip[3] + 1.) * vw * 0.5,
                vy + (v.clip[1] / v.clip[3] + 1.) * vh * 0.5,
                v.clip[2] / v.clip[3],
            ]
        });
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        if !area.is_finite() || area == 0. {
            continue;
        }
        let front = (area > 0.) == state.front_ccw;
        if state.cull == Cull::FrontAndBack
            || (front && state.cull == Cull::Front)
            || (!front && state.cull == Cull::Back)
        {
            continue;
        }
        let dzdx = (p[0][2] * (p[1][1] - p[2][1])
            + p[1][2] * (p[2][1] - p[0][1])
            + p[2][2] * (p[0][1] - p[1][1]))
            / area;
        let dzdy = (p[0][2] * (p[2][0] - p[1][0])
            + p[1][2] * (p[0][0] - p[2][0])
            + p[2][2] * (p[1][0] - p[0][0]))
            / area;
        let bias = state.polygon_offset[0]
            * dzdx.abs().max(dzdy.abs())
            * (state.depth.range[1] - state.depth.range[0]).abs()
            * 0.5
            + state.polygon_offset[1] / ((1u32 << 24) - 1) as f32;
        for v in tri {
            let mut packed = [0.; 16];
            packed[..4].copy_from_slice(&v.clip);
            // Apply depth range and per-triangle bias after interpolation in PS.
            packed[4] = v.fog;
            packed[5] = bias;
            packed[8..12].copy_from_slice(&v.color);
            packed[12..16].copy_from_slice(&v.uv);
            vertices.push(packed);
        }
    }
    s[362] = state.depth.range[0];
    s[363] = state.depth.range[1];
    if vertices.is_empty() {
        return Ok(None);
    }
    let flags = INDEXED_DRAW_LOAD_COLOR
        | if state.depth.enabled {
            INDEXED_DRAW_DRAWABLE_DEPTH
                | INDEXED_DRAW_DEPTH_TEST
                | if state.depth.write {
                    INDEXED_DRAW_DEPTH_WRITE
                } else {
                    0
                }
                | (compare(state.depth.func) << INDEXED_DRAW_DEPTH_COMPARE_SHIFT)
        } else {
            0
        };
    Ok(Some(Draw {
        vertices,
        state: s,
        pixels: atlas,
        texture_size,
        flags,
        clear_rgba8: 0,
    }))
}

pub(crate) fn clear(
    extent: [u32; 2],
    color: Option<[u8; 4]>,
    depth: Option<f32>,
    rect: Option<[i32; 4]>,
) -> Option<Draw> {
    if color.is_none() && depth.is_none() {
        return None;
    }
    let [left, top, right, bottom] = scissor(extent, rect)?;
    let x = |v: f32| 2. * v / extent[0] as f32 - 1.;
    let y = |v: f32| 1. - 2. * v / extent[1] as f32;
    let mut vertices = Vec::with_capacity(6);
    for [px, py] in [
        [left, top],
        [left, bottom],
        [right, top],
        [right, top],
        [left, bottom],
        [right, bottom],
    ] {
        let mut v = [0.; 16];
        v[..4].copy_from_slice(&[x(px), y(py), depth.unwrap_or(1.) * 2. - 1., 1.]);
        v[8..12].copy_from_slice(&color.unwrap_or([0;4]).map(|c|c as f32/255.));
        v[15] = 1.;
        vertices.push(v);
    }
    let mut state = base_state(extent);
    state[363] = 1.;
    Some(Draw {
        vertices,
        state,
        pixels: Arc::from([255u8; 4]),
        texture_size: [1, 1],
        flags: INDEXED_DRAW_GEOMETRY_CLEAR
            | if depth.is_some() {
                INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH
            } else {
                0
            }
            | if color.is_none() {
                INDEXED_DRAW_LOAD_COLOR
            } else {
                0
            },
        clear_rgba8: u32::from_le_bytes(color.unwrap_or([0; 4])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tri() -> [[ClipVertex; 3]; 1] {
        [
            [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|clip| ClipVertex {
                clip,
                color: [0.2, 0.4, 0.6, 0.8],
                uv: [2., 3., 0., 4.],
                fog: 7.,
            }),
        ]
    }
    fn state() -> RasterState {
        RasterState {
            viewport: [0, 0, 8, 8],
            ..Default::default()
        }
    }
    fn encode(s: &RasterState) -> Option<Draw> {
        draw([8, 8], &tri(), s, None, Arc::from([255; 4]), [1, 1]).unwrap()
    }
    #[test]
    fn prepared_attributes_preserve_cpu_lighting_projective_uv_and_fog() {
        let mut s = state();
        s.fog.enabled = true;
        s.fog.mode = FogMode::Exp2;
        s.depth.range = [0.25, 0.75];
        s.polygon_offset = [0., 2.];
        let d = encode(&s).unwrap();
        assert_eq!(d.vertices[0][..4], tri()[0][0].clip);
        assert_eq!(d.vertices[0][8..12], tri()[0][0].color);
        assert_eq!(d.vertices[0][12..16], [2., 3., 0., 4.]);
        assert_eq!(d.vertices[0][4], 7.);
        assert_eq!(d.vertices[0][5], 2. / ((1u32 << 24) - 1) as f32);
        assert_eq!(&d.state[362..364], &[0.25, 0.75]);
        assert_eq!(d.state[85], 0.); // Lighting is already complete.
        assert_eq!(d.state[360], 1.);
    }
    #[test]
    fn culling_and_empty_scissor_remove_work_without_reordering() {
        let mut s = state();
        s.cull = Cull::Front;
        assert!(encode(&s).is_none());
        s.cull = Cull::Back;
        assert_eq!(encode(&s).unwrap().vertices.len(), 3);
        s.front_ccw = false;
        assert!(encode(&s).is_none());
        s.cull = Cull::None;
        s.scissor_enabled = true;
        s.scissor = [9, 9, 1, 1];
        assert!(encode(&s).is_none());
    }
    #[test]
    fn clear_preserves_independent_color_and_depth_masks() {
        let d = clear([8, 8], None, Some(1.), Some([2, 1, 3, 4])).unwrap();
        assert_ne!(d.flags & INDEXED_DRAW_LOAD_COLOR, 0);
        assert_ne!(d.flags & INDEXED_DRAW_CLEAR_DEPTH, 0);
        assert!(indexed_draw_flags_valid(d.flags));
        let points: Vec<_> = d.vertices.iter().map(|v| [v[0], v[1]]).collect();
        assert!(
            points
                .iter()
                .all(|p| p[0] >= -0.5 && p[0] <= 0.25 && p[1] >= -0.75 && p[1] <= 0.25)
        );
        let color = clear([8, 8], Some([3, 4, 5, 6]), None, None).unwrap();
        assert_eq!(color.flags, INDEXED_DRAW_GEOMETRY_CLEAR);
        assert_eq!(color.clear_rgba8, u32::from_le_bytes([3, 4, 5, 6]));
        assert!(clear([8, 8], None, None, None).is_none());
    }
    #[test]
    fn viewport_and_depth_blend_state_are_packed_independently() {
        let mut s = state();
        s.viewport = [2, 1, 4, 6];
        s.depth.enabled = true;
        s.depth.func = Compare::Gequal;
        s.depth.write = false;
        s.blend.enabled = true;
        s.blend.src = BlendFactor::SrcColor;
        s.blend.dst = BlendFactor::OneMinusDstColor;
        let d = encode(&s).unwrap();
        assert_eq!(&d.state[108..112], &[0.5, 0.75, 0., 0.]);
        assert_eq!(
            d.flags & INDEXED_DRAW_DEPTH_COMPARE_MASK,
            6 << INDEXED_DRAW_DEPTH_COMPARE_SHIFT
        );
        assert_eq!(d.flags & INDEXED_DRAW_DEPTH_WRITE, 0);
        assert_eq!(
            [d.state[353], d.state[354], d.state[356], d.state[357]],
            [2., 21., 3., 20.]
        );
    }
}
