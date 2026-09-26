//! Small, deterministic fixed-function raster core used by the WC3 OpenGL
//! bridge.  It owns no guest state and no vGPU objects: the caller decodes GL
//! arrays/state into the types below, then presents `Frame::rgba` with the
//! existing sampled-UI4 path.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClipVertex {
    pub clip: [f32; 4],
    pub color: [f32; 4],
    /// Homogeneous texture coordinates (s, t, r, q).
    pub uv: [f32; 4],
    /// Eye-space fog coordinate, normally `abs(eye_z)`.
    pub fog: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextureLevel<'a> {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 rows, even when the GL internal format was RGB.
    pub rgba: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextureFormat {
    Alpha,
    Luminance,
    LuminanceAlpha,
    Rgb,
    Rgba,
    Intensity,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Wrap {
    Repeat,
    Clamp,
    ClampToEdge,
    MirroredRepeat,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Filter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextureView<'a> {
    pub levels: &'a [TextureLevel<'a>],
    pub format: TextureFormat,
    pub wrap_s: Wrap,
    pub wrap_t: Wrap,
    pub min_filter: Filter,
    pub mag_filter: Filter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Compare {
    Never,
    Less,
    Equal,
    Lequal,
    Greater,
    NotEqual,
    Gequal,
    Always,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cull {
    None,
    Front,
    Back,
    FrontAndBack,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TexEnvMode {
    Replace,
    Modulate,
    Decal,
    Blend,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FogMode {
    Linear,
    Exp,
    Exp2,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DepthState {
    pub enabled: bool,
    pub func: Compare,
    pub write: bool,
    pub range: [f32; 2],
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct AlphaState {
    pub enabled: bool,
    pub func: Compare,
    pub reference: f32,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlendState {
    pub enabled: bool,
    pub src: BlendFactor,
    pub dst: BlendFactor,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct FogState {
    pub enabled: bool,
    pub mode: FogMode,
    pub color: [f32; 4],
    pub density: f32,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterState {
    /// GL viewport coordinates: origin at the lower left of the framebuffer.
    pub viewport: [i32; 4],
    pub scissor_enabled: bool,
    /// GL scissor coordinates: origin at the lower left of the framebuffer.
    pub scissor: [i32; 4],
    pub cull: Cull,
    pub front_ccw: bool,
    pub depth: DepthState,
    pub alpha: AlphaState,
    pub blend: BlendState,
    pub polygon_offset: [f32; 2],
    pub fog: FogState,
    pub tex_env: TexEnvMode,
    pub tex_env_color: [f32; 4],
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            viewport: [0, 0, 0, 0],
            scissor_enabled: false,
            scissor: [0; 4],
            cull: Cull::None,
            front_ccw: true,
            depth: DepthState {
                enabled: false,
                func: Compare::Less,
                write: true,
                range: [0.0, 1.0],
            },
            alpha: AlphaState {
                enabled: false,
                func: Compare::Always,
                reference: 0.0,
            },
            blend: BlendState {
                enabled: false,
                src: BlendFactor::One,
                dst: BlendFactor::Zero,
            },
            polygon_offset: [0.0, 0.0],
            fog: FogState {
                enabled: false,
                mode: FogMode::Linear,
                color: [0.0, 0.0, 0.0, 0.0],
                density: 1.0,
                start: 0.0,
                end: 1.0,
            },
            tex_env: TexEnvMode::Modulate,
            tex_env_color: [0.0; 4],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub depth: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RasterStats {
    pub input_triangles: u32,
    pub clipped_triangles: u32,
    pub shaded_pixels: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RasterError {
    BadExtent,
    PixelBudget,
    BadVertex,
    BadIndex,
    BadTexture,
    UnsupportedTexEnv,
    BadViewport,
}

pub(crate) const MAX_RASTER_PIXELS: usize = 1920 * 1080 * 2;

// Stable bridge-facing names.  The short internal names keep the clipping and
// sampling code readable; these are the names used by `staticgl.rs`.
pub(crate) type GlRasterVertex = ClipVertex;
pub(crate) type GlRasterLevel<'a> = TextureLevel<'a>;
pub(crate) type GlRasterTexture<'a> = TextureView<'a>;
pub(crate) type GlRasterState = RasterState;
pub(crate) type GlRasterFrame = Frame;
pub(crate) type GlRasterStats = RasterStats;

impl Frame {
    /// Creates a bounded, bottom-up OpenGL framebuffer.  `rgba` row zero is
    /// the GL lower row; the UI4 presenter deliberately flips it once.
    pub(crate) fn new(width: u32, height: u32) -> Result<Self, &'static str> {
        Self::new_with_budget(width, height, MAX_RASTER_PIXELS).map_err(raster_error)
    }

    pub(crate) fn new_with_budget(
        width: u32,
        height: u32,
        max_pixels: usize,
    ) -> Result<Self, RasterError> {
        let pixels = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_| RasterError::BadExtent)?;
        if width == 0 || height == 0 {
            return Err(RasterError::BadExtent);
        }
        if pixels > max_pixels {
            return Err(RasterError::PixelBudget);
        }
        Ok(Self {
            width,
            height,
            rgba: vec![0; pixels.checked_mul(4).ok_or(RasterError::BadExtent)?],
            depth: vec![1.0; pixels],
        })
    }

    /// Applies GL `Clear` semantics to the whole frame or a lower-left scissor
    /// rectangle.  Passing `None` leaves that attachment unchanged.
    pub(crate) fn clear(
        &mut self,
        color: Option<[u8; 4]>,
        depth: Option<f32>,
        scissor: Option<[i32; 4]>,
    ) {
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if scissor.is_some_and(|s| !in_scissor_gl(s, x, y)) {
                    continue;
                }
                let i = self.gl_index(x, y);
                if let Some(color) = color {
                    self.rgba[i * 4..i * 4 + 4].copy_from_slice(&color);
                }
                if let Some(depth) = depth {
                    self.depth[i] = depth;
                }
            }
        }
    }

    /// Rasterizes one indexed triangle list.  All clipping and bounds checks
    /// complete before the first framebuffer write, so a rejected draw cannot
    /// leave a partially rendered frame behind.
    pub(crate) fn draw_indexed(
        &mut self,
        state: &RasterState,
        texture: Option<TextureView<'_>>,
        vertices: &[ClipVertex],
        indices: &[u32],
    ) -> Result<RasterStats, RasterError> {
        if indices.len() % 3 != 0 {
            return Err(RasterError::BadIndex);
        }
        validate_state(self, state)?;
        if state.viewport[2] == 0 || state.viewport[3] == 0 {
            return Ok(RasterStats {
                input_triangles: (indices.len() / 3) as u32,
                ..RasterStats::default()
            });
        }
        if let Some(texture) = texture {
            validate_texture(texture)?;
            if !texenv_supported(state.tex_env, texture.format) {
                return Err(RasterError::UnsupportedTexEnv);
            }
        }
        if vertices.iter().any(|v| !vertex_valid(*v)) {
            return Err(RasterError::BadVertex);
        }
        if indices.iter().any(|&i| i as usize >= vertices.len()) {
            return Err(RasterError::BadIndex);
        }
        let mut staged = Vec::with_capacity(indices.len() / 3);
        for tri in indices.chunks_exact(3) {
            let polygon = clip_triangle([
                vertices[tri[0] as usize],
                vertices[tri[1] as usize],
                vertices[tri[2] as usize],
            ]);
            for n in 1..polygon.len().saturating_sub(1) {
                staged.push([polygon[0], polygon[n], polygon[n + 1]]);
            }
        }
        let mut stats = RasterStats {
            input_triangles: (indices.len() / 3) as u32,
            clipped_triangles: staged.len() as u32,
            shaded_pixels: 0,
        };
        for tri in staged {
            stats.shaded_pixels += self.draw_triangle(state, texture, tri) as u64;
        }
        Ok(stats)
    }

    pub(crate) fn draw_triangles(
        &mut self,
        vertices: &[GlRasterVertex],
        indices: &[u32],
        state: &GlRasterState,
        texture: Option<&GlRasterTexture<'_>>,
    ) -> Result<GlRasterStats, &'static str> {
        self.draw_indexed(state, texture.copied(), vertices, indices)
            .map_err(raster_error)
    }

    fn draw_triangle(
        &mut self,
        state: &RasterState,
        texture: Option<TextureView<'_>>,
        tri: [ClipVertex; 3],
    ) -> usize {
        let mut p = tri.map(|v| ScreenVertex::from_clip(v, state.viewport, self.height));
        let area_gl = orient([p[0].x, -p[0].y], [p[1].x, -p[1].y], [p[2].x, -p[2].y]);
        if area_gl == 0.0 {
            return 0;
        }
        let front = if state.front_ccw {
            area_gl > 0.0
        } else {
            area_gl < 0.0
        };
        if matches!(state.cull, Cull::FrontAndBack)
            || (front && matches!(state.cull, Cull::Front))
            || (!front && matches!(state.cull, Cull::Back))
        {
            return 0;
        }
        let mut area = orient([p[0].x, p[0].y], [p[1].x, p[1].y], [p[2].x, p[2].y]);
        if area < 0.0 {
            p.swap(1, 2);
            area = -area;
        }
        // Window-depth is affine in screen coordinates.  GL's polygon offset
        // uses its maximum slope plus `units * r`, where r is one minimum
        // resolvable depth increment.  UI4 ultimately stores floats, but the
        // fixed GL bridge models the conventional 24-bit depth precision.
        let dzdx = (p[0].z_ndc * (p[1].y - p[2].y)
            + p[1].z_ndc * (p[2].y - p[0].y)
            + p[2].z_ndc * (p[0].y - p[1].y))
            / area;
        let dzdy = (p[0].z_ndc * (p[2].x - p[1].x)
            + p[1].z_ndc * (p[0].x - p[2].x)
            + p[2].z_ndc * (p[1].x - p[0].x))
            / area;
        let range_scale = (state.depth.range[1] - state.depth.range[0]).abs() * 0.5;
        let polygon_bias = state.polygon_offset[0] * dzdx.abs().max(dzdy.abs()) * range_scale
            + state.polygon_offset[1] / ((1u32 << 24) - 1) as f32;
        let min_x = p
            .iter()
            .map(|v| v.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as i32;
        let max_x = p
            .iter()
            .map(|v| v.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width as f32 - 1.0) as i32;
        let min_y = p
            .iter()
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as i32;
        let max_y = p
            .iter()
            .map(|v| v.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height as f32 - 1.0) as i32;
        let mut shaded = 0;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let at = [x as f32 + 0.5, y as f32 + 0.5];
                let e0 = orient([p[1].x, p[1].y], [p[2].x, p[2].y], at);
                let e1 = orient([p[2].x, p[2].y], [p[0].x, p[0].y], at);
                let e2 = orient([p[0].x, p[0].y], [p[1].x, p[1].y], at);
                if !edge_inside(e0, p[1], p[2])
                    || !edge_inside(e1, p[2], p[0])
                    || !edge_inside(e2, p[0], p[1])
                {
                    continue;
                }
                if state.scissor_enabled && !in_scissor(self.height, state.scissor, x, y) {
                    continue;
                }
                let b = [e0 / area, e1 / area, e2 / area];
                let depth = (state.depth.range[0]
                    + ((b[0] * p[0].z_ndc + b[1] * p[1].z_ndc + b[2] * p[2].z_ndc) * 0.5 + 0.5)
                        * (state.depth.range[1] - state.depth.range[0])
                    + polygon_bias)
                    .clamp(0.0, 1.0);
                let offset = self.top_index(x, y);
                if state.depth.enabled && !compare(state.depth.func, depth, self.depth[offset]) {
                    continue;
                }
                let mut color = perspective_color(&p, b);
                if let Some(texture) = texture {
                    let uv = perspective_uv(&p, b);
                    let uv_dx = perspective_uv_at(&p, [x as f32 + 1.5, y as f32 + 0.5]);
                    let uv_dy = perspective_uv_at(&p, [x as f32 + 0.5, y as f32 + 1.5]);
                    let sample = sample_texture(texture, uv, uv_dx, uv_dy);
                    color = texenv(
                        state.tex_env,
                        state.tex_env_color,
                        texture.format,
                        color,
                        sample,
                    );
                }
                if state.fog.enabled {
                    color = apply_fog(state.fog, perspective_fog(&p, b), color);
                }
                color = color.map(clamp01);
                if state.alpha.enabled
                    && !compare(state.alpha.func, color[3], state.alpha.reference)
                {
                    continue;
                }
                let dst = read_pixel(&self.rgba, offset);
                let out = if state.blend.enabled {
                    blend(state.blend, color, dst)
                } else {
                    color
                };
                write_pixel(&mut self.rgba, offset, out);
                if state.depth.enabled && state.depth.write {
                    self.depth[offset] = depth;
                }
                shaded += 1;
            }
        }
        shaded
    }

    /// Converts the raster's top-down coverage coordinate into its bottom-up
    /// GL backing row.
    fn top_index(&self, x: i32, y_top: i32) -> usize {
        (self.height as usize - 1 - y_top as usize) * self.width as usize + x as usize
    }
    fn gl_index(&self, x: i32, y_gl: i32) -> usize {
        y_gl as usize * self.width as usize + x as usize
    }
}

#[derive(Clone, Copy)]
struct ScreenVertex {
    x: f32,
    y: f32,
    z_ndc: f32,
    inv_w: f32,
    color_over_w: [f32; 4],
    uv_over_w: [f32; 4],
    fog_over_w: f32,
}
impl ScreenVertex {
    fn from_clip(v: ClipVertex, viewport: [i32; 4], frame_height: u32) -> Self {
        let inv_w = 1.0 / v.clip[3];
        let nx = v.clip[0] * inv_w;
        let ny = v.clip[1] * inv_w;
        Self {
            x: viewport[0] as f32 + (nx + 1.0) * viewport[2] as f32 * 0.5,
            y: frame_height as f32 - (viewport[1] as f32 + (ny + 1.0) * viewport[3] as f32 * 0.5),
            z_ndc: v.clip[2] * inv_w,
            inv_w,
            color_over_w: v.color.map(|c| c * inv_w),
            uv_over_w: v.uv.map(|c| c * inv_w),
            fog_over_w: v.fog * inv_w,
        }
    }
}

fn validate_state(_frame: &Frame, state: &RasterState) -> Result<(), RasterError> {
    let [_, _, w, h] = state.viewport;
    if w < 0 || h < 0 || !state.depth.range.iter().all(|v| v.is_finite()) {
        return Err(RasterError::BadViewport);
    }
    Ok(())
}
fn validate_texture(texture: TextureView<'_>) -> Result<(), RasterError> {
    let Some(base) = texture.levels.first() else {
        return Err(RasterError::BadTexture);
    };
    if base.width == 0
        || base.height == 0
        || base.rgba.len() != base.width as usize * base.height as usize * 4
    {
        return Err(RasterError::BadTexture);
    }
    let mip_filter = matches!(
        texture.min_filter,
        Filter::NearestMipmapNearest
            | Filter::LinearMipmapNearest
            | Filter::NearestMipmapLinear
            | Filter::LinearMipmapLinear
    );
    if !mip_filter {
        return Ok(());
    }
    let mut width = base.width;
    let mut height = base.height;
    let expected = (width.max(height).ilog2() + 1) as usize;
    if texture.levels.len() != expected {
        return Err(RasterError::BadTexture);
    }
    for level in texture.levels {
        if level.width != width
            || level.height != height
            || level.rgba.len() != width as usize * height as usize * 4
        {
            return Err(RasterError::BadTexture);
        }
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok(())
}
fn vertex_valid(v: ClipVertex) -> bool {
    v.clip
        .iter()
        .chain(v.color.iter())
        .chain(v.uv.iter())
        .chain([v.fog].iter())
        .all(|x| x.is_finite())
}
fn clip_triangle(input: [ClipVertex; 3]) -> Vec<ClipVertex> {
    let mut polygon = input.to_vec();
    // The six canonical clip half-spaces imply w >= 0.  A point exactly at
    // the homogeneous origin satisfies them all but cannot be divided.  Clip
    // it to a small positive-w plane before the perspective divide; this is
    // a finite representation of the same limiting primitive.
    for plane in 0..7 {
        if polygon.is_empty() {
            break;
        }
        let old = core::mem::take(&mut polygon);
        for i in 0..old.len() {
            let a = old[i];
            let b = old[(i + 1) % old.len()];
            let da = plane_distance(a.clip, plane);
            let db = plane_distance(b.clip, plane);
            let ina = da >= 0.0;
            let inb = db >= 0.0;
            if ina {
                polygon.push(a);
            }
            if ina != inb {
                polygon.push(lerp_vertex(a, b, da / (da - db)));
            }
        }
    }
    polygon
}
fn plane_distance(p: [f32; 4], n: usize) -> f32 {
    match n {
        0 => p[0] + p[3],
        1 => p[3] - p[0],
        2 => p[1] + p[3],
        3 => p[3] - p[1],
        4 => p[2] + p[3],
        5 => p[3] - p[2],
        _ => p[3] - 1.0e-6,
    }
}
fn lerp_vertex(a: ClipVertex, b: ClipVertex, t: f32) -> ClipVertex {
    ClipVertex {
        clip: lerp4(a.clip, b.clip, t),
        color: lerp4(a.color, b.color, t),
        uv: lerp4(a.uv, b.uv, t),
        fog: a.fog + (b.fog - a.fog) * t,
    }
}
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    core::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn orient(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn edge_inside(edge: f32, a: ScreenVertex, b: ScreenVertex) -> bool {
    edge > 0.0 || (edge == 0.0 && ((b.y - a.y) < 0.0 || ((b.y - a.y) == 0.0 && (b.x - a.x) > 0.0)))
}
fn in_scissor(height: u32, s: [i32; 4], x: i32, y: i32) -> bool {
    in_scissor_gl(s, x, height as i32 - 1 - y)
}
fn in_scissor_gl(s: [i32; 4], x: i32, y: i32) -> bool {
    let [left, bottom, width, height] = s.map(i64::from);
    width > 0
        && height > 0
        && i64::from(x) >= left
        && i64::from(x) < left + width
        && i64::from(y) >= bottom
        && i64::from(y) < bottom + height
}
fn perspective_weight(p: &[ScreenVertex; 3], b: [f32; 3]) -> [f32; 3] {
    let d = b[0] * p[0].inv_w + b[1] * p[1].inv_w + b[2] * p[2].inv_w;
    [
        b[0] * p[0].inv_w / d,
        b[1] * p[1].inv_w / d,
        b[2] * p[2].inv_w / d,
    ]
}
fn perspective_color(p: &[ScreenVertex; 3], b: [f32; 3]) -> [f32; 4] {
    let w = perspective_weight(p, b);
    core::array::from_fn(|i| {
        w[0] * (p[0].color_over_w[i] / p[0].inv_w)
            + w[1] * (p[1].color_over_w[i] / p[1].inv_w)
            + w[2] * (p[2].color_over_w[i] / p[2].inv_w)
    })
}
fn perspective_fog(p: &[ScreenVertex; 3], b: [f32; 3]) -> f32 {
    let w = perspective_weight(p, b);
    w[0] * (p[0].fog_over_w / p[0].inv_w)
        + w[1] * (p[1].fog_over_w / p[1].inv_w)
        + w[2] * (p[2].fog_over_w / p[2].inv_w)
}
fn perspective_uv(p: &[ScreenVertex; 3], b: [f32; 3]) -> [f32; 4] {
    let w = perspective_weight(p, b);
    let q: [f32; 4] = core::array::from_fn(|i| {
        w[0] * (p[0].uv_over_w[i] / p[0].inv_w)
            + w[1] * (p[1].uv_over_w[i] / p[1].inv_w)
            + w[2] * (p[2].uv_over_w[i] / p[2].inv_w)
    });
    [q[0] / q[3], q[1] / q[3], q[2] / q[3], q[3]]
}
fn perspective_uv_at(p: &[ScreenVertex; 3], at: [f32; 2]) -> [f32; 4] {
    let area = orient([p[0].x, p[0].y], [p[1].x, p[1].y], [p[2].x, p[2].y]);
    if area == 0.0 {
        return [0.0; 4];
    }
    perspective_uv(
        p,
        [
            orient([p[1].x, p[1].y], [p[2].x, p[2].y], at) / area,
            orient([p[2].x, p[2].y], [p[0].x, p[0].y], at) / area,
            orient([p[0].x, p[0].y], [p[1].x, p[1].y], at) / area,
        ],
    )
}

fn sample_texture(tex: TextureView<'_>, uv: [f32; 4], dx: [f32; 4], dy: [f32; 4]) -> [f32; 4] {
    let base = &tex.levels[0];
    let du = ((dx[0] - uv[0]) * base.width as f32).hypot((dy[0] - uv[0]) * base.width as f32);
    let dv = ((dx[1] - uv[1]) * base.height as f32).hypot((dy[1] - uv[1]) * base.height as f32);
    let lod = du.max(dv).max(1.0).log2();
    let mag = lod <= 0.0;
    let filter = if mag { tex.mag_filter } else { tex.min_filter };
    let (a, b, t, linear) = match filter {
        Filter::Nearest => (0, 0, 0.0, false),
        Filter::Linear => (0, 0, 0.0, true),
        Filter::NearestMipmapNearest => {
            let n = lod.round().max(0.0) as usize;
            (n, n, 0.0, false)
        }
        Filter::LinearMipmapNearest => {
            let n = lod.round().max(0.0) as usize;
            (n, n, 0.0, true)
        }
        Filter::NearestMipmapLinear => {
            let a = lod.floor().max(0.0) as usize;
            (a, a + 1, lod - a as f32, false)
        }
        Filter::LinearMipmapLinear => {
            let a = lod.floor().max(0.0) as usize;
            (a, a + 1, lod - a as f32, true)
        }
    };
    let a = a.min(tex.levels.len() - 1);
    let b = b.min(tex.levels.len() - 1);
    let ca = sample_level(tex.levels[a], uv, tex.wrap_s, tex.wrap_t, linear);
    let cb = sample_level(tex.levels[b], uv, tex.wrap_s, tex.wrap_t, linear);
    lerp4(ca, cb, t)
}
fn sample_level(
    level: TextureLevel<'_>,
    uv: [f32; 4],
    ws: Wrap,
    wt: Wrap,
    linear: bool,
) -> [f32; 4] {
    let x = wrap_coord(uv[0], ws) * level.width as f32 - 0.5;
    let y = wrap_coord(uv[1], wt) * level.height as f32 - 0.5;
    if !linear {
        return fetch(level, x.round() as i32, y.round() as i32, ws, wt);
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let a = lerp4(
        fetch(level, x0, y0, ws, wt),
        fetch(level, x0 + 1, y0, ws, wt),
        fx,
    );
    let b = lerp4(
        fetch(level, x0, y0 + 1, ws, wt),
        fetch(level, x0 + 1, y0 + 1, ws, wt),
        fx,
    );
    lerp4(a, b, fy)
}
fn wrap_coord(v: f32, w: Wrap) -> f32 {
    match w {
        Wrap::Repeat => v - v.floor(),
        Wrap::MirroredRepeat => {
            let f = v.floor();
            if (f as i32) & 1 == 0 {
                v - f
            } else {
                1.0 - (v - f)
            }
        }
        Wrap::Clamp => v,
        Wrap::ClampToEdge => v.clamp(0.0, 1.0),
    }
}
fn fetch(l: TextureLevel<'_>, x: i32, y: i32, ws: Wrap, wt: Wrap) -> [f32; 4] {
    if (ws == Wrap::Clamp && !(0..l.width as i32).contains(&x))
        || (wt == Wrap::Clamp && !(0..l.height as i32).contains(&y))
    {
        return [0.0; 4];
    }
    let ix = wrap_index(x, l.width as i32, ws);
    let iy = wrap_index(y, l.height as i32, wt);
    let p = &l.rgba[(iy as usize * l.width as usize + ix as usize) * 4..][..4];
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}
fn wrap_index(i: i32, n: i32, w: Wrap) -> i32 {
    match w {
        Wrap::Repeat => i.rem_euclid(n),
        Wrap::MirroredRepeat => {
            let q = i.div_euclid(n);
            let r = i.rem_euclid(n);
            if q & 1 == 0 { r } else { n - 1 - r }
        }
        Wrap::Clamp | Wrap::ClampToEdge => i.clamp(0, n - 1),
    }
}
fn texenv(
    mode: TexEnvMode,
    env: [f32; 4],
    fmt: TextureFormat,
    p: [f32; 4],
    t: [f32; 4],
) -> [f32; 4] {
    match mode {
        TexEnvMode::Modulate => match fmt {
            TextureFormat::Alpha => [p[0], p[1], p[2], p[3] * t[3]],
            TextureFormat::Luminance => [p[0] * t[0], p[1] * t[0], p[2] * t[0], p[3]],
            TextureFormat::LuminanceAlpha => [p[0] * t[0], p[1] * t[0], p[2] * t[0], p[3] * t[3]],
            TextureFormat::Rgb => [p[0] * t[0], p[1] * t[1], p[2] * t[2], p[3]],
            TextureFormat::Rgba => core::array::from_fn(|i| p[i] * t[i]),
            TextureFormat::Intensity => core::array::from_fn(|i| p[i] * t[0]),
        },
        TexEnvMode::Replace => match fmt {
            TextureFormat::Alpha => [p[0], p[1], p[2], t[3]],
            TextureFormat::Luminance => [t[0], t[0], t[0], p[3]],
            TextureFormat::LuminanceAlpha => [t[0], t[0], t[0], t[3]],
            TextureFormat::Rgb => [t[0], t[1], t[2], p[3]],
            TextureFormat::Rgba => t,
            TextureFormat::Intensity => [t[0], t[0], t[0], t[0]],
        },
        TexEnvMode::Decal => match fmt {
            TextureFormat::Rgb => [t[0], t[1], t[2], p[3]],
            TextureFormat::Rgba => [
                p[0] * (1.0 - t[3]) + t[0] * t[3],
                p[1] * (1.0 - t[3]) + t[1] * t[3],
                p[2] * (1.0 - t[3]) + t[2] * t[3],
                p[3],
            ],
            // draw_indexed rejects these per GL 1.1 table 3.10.
            TextureFormat::Alpha
            | TextureFormat::Luminance
            | TextureFormat::LuminanceAlpha
            | TextureFormat::Intensity => p,
        },
        TexEnvMode::Blend => {
            let luminance = matches!(
                fmt,
                TextureFormat::Luminance | TextureFormat::LuminanceAlpha | TextureFormat::Intensity
            );
            let alpha = match fmt {
                TextureFormat::Alpha
                | TextureFormat::LuminanceAlpha
                | TextureFormat::Rgba
                | TextureFormat::Intensity => p[3] * t[3],
                _ => p[3],
            };
            if fmt == TextureFormat::Alpha {
                [p[0], p[1], p[2], alpha]
            } else {
                let tc = if luminance {
                    [t[0]; 3]
                } else {
                    [t[0], t[1], t[2]]
                };
                [
                    p[0] * (1.0 - tc[0]) + env[0] * tc[0],
                    p[1] * (1.0 - tc[1]) + env[1] * tc[1],
                    p[2] * (1.0 - tc[2]) + env[2] * tc[2],
                    alpha,
                ]
            }
        }
    }
}

fn texenv_supported(mode: TexEnvMode, format: TextureFormat) -> bool {
    !matches!(mode, TexEnvMode::Decal) || matches!(format, TextureFormat::Rgb | TextureFormat::Rgba)
}
fn apply_fog(f: FogState, z: f32, c: [f32; 4]) -> [f32; 4] {
    let q = match f.mode {
        FogMode::Linear => {
            if f.end == f.start {
                0.0
            } else {
                (f.end - z) / (f.end - f.start)
            }
        }
        FogMode::Exp => (-f.density * z).exp(),
        FogMode::Exp2 => {
            let d = f.density * z;
            (-d * d).exp()
        }
    }
    .clamp(0.0, 1.0);
    [
        c[0] * q + f.color[0] * (1.0 - q),
        c[1] * q + f.color[1] * (1.0 - q),
        c[2] * q + f.color[2] * (1.0 - q),
        c[3],
    ]
}
fn compare(c: Compare, a: f32, b: f32) -> bool {
    match c {
        Compare::Never => false,
        Compare::Less => a < b,
        Compare::Equal => a == b,
        Compare::Lequal => a <= b,
        Compare::Greater => a > b,
        Compare::NotEqual => a != b,
        Compare::Gequal => a >= b,
        Compare::Always => true,
    }
}
fn blend(s: BlendState, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let sf = factor(s.src, src, dst);
    let df = factor(s.dst, src, dst);
    core::array::from_fn(|i| clamp01(src[i] * sf[i] + dst[i] * df[i]))
}
fn factor(f: BlendFactor, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    match f {
        BlendFactor::Zero => [0.0; 4],
        BlendFactor::One => [1.0; 4],
        BlendFactor::SrcColor => s,
        BlendFactor::OneMinusSrcColor => s.map(|x| 1.0 - x),
        BlendFactor::DstColor => d,
        BlendFactor::OneMinusDstColor => d.map(|x| 1.0 - x),
        BlendFactor::SrcAlpha => [s[3]; 4],
        BlendFactor::OneMinusSrcAlpha => [1.0 - s[3]; 4],
        BlendFactor::DstAlpha => [d[3]; 4],
        BlendFactor::OneMinusDstAlpha => [1.0 - d[3]; 4],
        BlendFactor::SrcAlphaSaturate => {
            let rgb = s[3].min(1.0 - d[3]);
            [rgb, rgb, rgb, 1.0]
        }
    }
}
fn read_pixel(b: &[u8], i: usize) -> [f32; 4] {
    let p = &b[i * 4..i * 4 + 4];
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}
fn write_pixel(b: &mut [u8], i: usize, c: [f32; 4]) {
    for k in 0..4 {
        b[i * 4 + k] = (clamp01(c[k]) * 255.0).round() as u8;
    }
}
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}
fn raster_error(error: RasterError) -> &'static str {
    match error {
        RasterError::BadExtent => "raster bad extent",
        RasterError::PixelBudget => "raster pixel budget",
        RasterError::BadVertex => "raster nonfinite vertex",
        RasterError::BadIndex => "raster bad index list",
        RasterError::BadTexture => "raster bad texture",
        RasterError::UnsupportedTexEnv => "raster undefined texture environment",
        RasterError::BadViewport => "raster bad viewport",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(x: f32, y: f32, z: f32, w: f32, u: f32, v: f32, c: [f32; 4]) -> ClipVertex {
        ClipVertex {
            clip: [x, y, z, w],
            color: c,
            uv: [u, v, 0.0, 1.0],
            fog: 0.0,
        }
    }
    fn state() -> RasterState {
        RasterState {
            viewport: [0, 0, 8, 8],
            depth: DepthState {
                enabled: true,
                func: Compare::Less,
                write: true,
                range: [0.0, 1.0],
            },
            ..Default::default()
        }
    }
    #[test]
    fn perspective_clip_and_depth_overlap() {
        let mut f = Frame::new(8, 8).unwrap();
        let a = [
            v(-2., -1., 0., 1., 0., 0., [1.; 4]),
            v(2., -1., 0., 1., 1., 0., [1.; 4]),
            v(0., 2., 0., 2., 0.5, 1., [1.; 4]),
        ];
        assert!(
            f.draw_indexed(&state(), None, &a, &[0, 1, 2])
                .unwrap()
                .shaded_pixels
                > 0
        );
        let near = [
            v(-1., -1., -1., 1., 0., 0., [0., 1., 0., 1.]),
            v(1., -1., -1., 1., 1., 0., [0., 1., 0., 1.]),
            v(0., 1., -1., 1., 0.5, 1., [0., 1., 0., 1.]),
        ];
        f.draw_indexed(&state(), None, &near, &[0, 1, 2]).unwrap();
        assert!(f.rgba.chunks_exact(4).any(|p| p[1] > p[0]));
    }
    #[test]
    fn mip_sampling_and_scissor() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let mip = [0, 0, 255, 255];
        let levels = [
            TextureLevel {
                width: 2,
                height: 2,
                rgba: &rgba,
            },
            TextureLevel {
                width: 1,
                height: 1,
                rgba: &mip,
            },
        ];
        let mut f = Frame::new(8, 8).unwrap();
        let mut s = state();
        s.scissor_enabled = true;
        s.scissor = [2, 2, 2, 2];
        let t = TextureView {
            levels: &levels,
            format: TextureFormat::Rgba,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
            min_filter: Filter::LinearMipmapLinear,
            mag_filter: Filter::Nearest,
        };
        let tri = [
            v(-1., -1., 0., 1., 0., 0., [1.; 4]),
            v(1., -1., 0., 1., 8., 0., [1.; 4]),
            v(-1., 1., 0., 1., 0., 8., [1.; 4]),
        ];
        f.draw_indexed(&s, Some(t), &tri, &[0, 1, 2]).unwrap();
        assert_eq!(&f.rgba[..4], [0, 0, 0, 0]);
        assert!(f.rgba.chunks_exact(4).any(|p| p[3] != 0));
    }

    #[test]
    fn perspective_uv_uses_clip_w() {
        let state = state();
        let p = [
            ScreenVertex::from_clip(v(-1., -1., 0., 1., 0., 0., [1.; 4]), state.viewport, 8),
            ScreenVertex::from_clip(v(1., -1., 0., 2., 1., 0., [1.; 4]), state.viewport, 8),
            ScreenVertex::from_clip(v(-1., 1., 0., 1., 0., 1., [1.; 4]), state.viewport, 8),
        ];
        // Equal screen-space weights do not mean equal attribute weights when
        // the second vertex has w=2.
        let uv = perspective_uv(&p, [0.5, 0.5, 0.0]);
        assert!((uv[0] - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn clear_depth_only_honors_lower_left_scissor() {
        let mut frame = Frame::new(4, 4).unwrap();
        frame.rgba.fill(77);
        frame.depth.fill(0.25);
        frame.clear(None, Some(0.75), Some([1, 0, 2, 1]));
        assert_eq!(frame.rgba[0], 77);
        assert_eq!(frame.depth[1], 0.75);
        assert_eq!(frame.depth[2], 0.75);
        assert_eq!(frame.depth[0], 0.25);
        assert_eq!(frame.depth[4], 0.25);
    }

    #[test]
    fn rgb_replace_preserves_primary_alpha_and_half_alpha_blends() {
        assert_eq!(
            texenv(
                TexEnvMode::Replace,
                [0.; 4],
                TextureFormat::Rgb,
                [0.1, 0.2, 0.3, 0.25],
                [0.9, 0.8, 0.7, 1.0]
            ),
            [0.9, 0.8, 0.7, 0.25]
        );
        let result = blend(
            BlendState {
                enabled: true,
                src: BlendFactor::SrcAlpha,
                dst: BlendFactor::OneMinusSrcAlpha,
            },
            [1.0, 0.0, 0.0, 0.5],
            [0.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(result, [0.5, 0.0, 0.0, 0.75]);
    }

    #[test]
    fn texture_env_base_formats_follow_gl11_tables() {
        let p = [0.2, 0.4, 0.6, 0.8];
        let la = [0.25, 0.25, 0.25, 0.5];
        let rgba = [0.9, 0.7, 0.5, 0.25];
        let env = [1.0, 0.0, 0.5, 1.0];
        assert_eq!(
            texenv(TexEnvMode::Modulate, env, TextureFormat::Alpha, p, la),
            [0.2, 0.4, 0.6, 0.4]
        );
        assert_eq!(
            texenv(TexEnvMode::Replace, env, TextureFormat::Alpha, p, la),
            [0.2, 0.4, 0.6, 0.5]
        );
        assert_eq!(
            texenv(TexEnvMode::Replace, env, TextureFormat::Luminance, p, la),
            [0.25, 0.25, 0.25, 0.8]
        );
        assert_eq!(
            texenv(
                TexEnvMode::Modulate,
                env,
                TextureFormat::LuminanceAlpha,
                p,
                la
            ),
            [0.05, 0.1, 0.15, 0.4]
        );
        assert_eq!(
            texenv(TexEnvMode::Replace, env, TextureFormat::Intensity, p, la),
            [0.25, 0.25, 0.25, 0.25]
        );
        assert_eq!(
            texenv(TexEnvMode::Replace, env, TextureFormat::Rgb, p, rgba),
            [0.9, 0.7, 0.5, 0.8]
        );
        assert_eq!(
            texenv(TexEnvMode::Replace, env, TextureFormat::Rgba, p, rgba),
            rgba
        );
        assert_eq!(
            texenv(TexEnvMode::Decal, env, TextureFormat::Rgb, p, rgba),
            [0.9, 0.7, 0.5, 0.8]
        );
        assert!(!texenv_supported(
            TexEnvMode::Decal,
            TextureFormat::LuminanceAlpha
        ));
        assert!(texenv_supported(
            TexEnvMode::Blend,
            TextureFormat::Intensity
        ));
    }

    #[test]
    fn homogeneous_origin_is_clipped_to_positive_w_without_nan() {
        let mut frame = Frame::new(8, 8).unwrap();
        let tri = [
            v(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, [1.; 4]),
            v(-0.8, -0.8, 0.0, 1.0, 0.0, 0.0, [1.; 4]),
            v(0.8, -0.8, 0.0, 1.0, 1.0, 0.0, [1.; 4]),
        ];
        let result = frame
            .draw_indexed(&state(), None, &tri, &[0, 1, 2])
            .unwrap();
        assert!(result.clipped_triangles > 0);
        assert!(frame.depth.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn offset_viewport_uses_frame_origin_not_viewport_top() {
        let mut frame = Frame::new(8, 8).unwrap();
        let state = RasterState {
            viewport: [2, 1, 4, 4],
            ..state()
        };
        let quad = [
            v(-1., -1., 0., 1., 0., 0., [1., 0., 0., 1.]),
            v(1., -1., 0., 1., 1., 0., [1., 0., 0., 1.]),
            v(1., 1., 0., 1., 1., 1., [1., 0., 0., 1.]),
            v(-1., 1., 0., 1., 0., 1., [1., 0., 0., 1.]),
        ];
        frame
            .draw_indexed(&state, None, &quad, &[0, 1, 2, 0, 2, 3])
            .unwrap();
        // `rgba` is bottom-up.  The viewport occupies x=[2,6), y=[1,5).
        assert_eq!(frame.rgba[(1 * 8 + 2) * 4], 255);
        assert_eq!(frame.rgba[(0 * 8 + 2) * 4], 0);
        assert_eq!(frame.rgba[(5 * 8 + 2) * 4], 0);
    }

    #[test]
    fn zero_and_offscreen_viewports_are_valid_noops() {
        let vertices = [
            v(-1., -1., 0., 1., 0., 0., [1.; 4]),
            v(1., -1., 0., 1., 1., 0., [1.; 4]),
            v(-1., 1., 0., 1., 0., 1., [1.; 4]),
        ];
        let mut frame = Frame::new(8, 8).unwrap();
        let zero = RasterState {
            viewport: [0, 0, 0, 8],
            ..state()
        };
        assert_eq!(
            frame
                .draw_indexed(&zero, None, &vertices, &[0, 1, 2])
                .unwrap()
                .shaded_pixels,
            0
        );
        let offscreen = RasterState {
            viewport: [-20, -20, 4, 4],
            ..state()
        };
        assert_eq!(
            frame
                .draw_indexed(&offscreen, None, &vertices, &[0, 1, 2])
                .unwrap()
                .shaded_pixels,
            0
        );
        assert!(frame.rgba.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn shared_edge_quad_has_no_hole_or_double_blend() {
        let mut frame = Frame::new(8, 8).unwrap();
        let mut s = state();
        s.depth.enabled = false;
        s.blend = BlendState {
            enabled: true,
            src: BlendFactor::SrcAlpha,
            dst: BlendFactor::OneMinusSrcAlpha,
        };
        let vertices = [
            v(-1., -1., 0., 1., 0., 0., [1., 0., 0., 0.5]),
            v(1., -1., 0., 1., 1., 0., [1., 0., 0., 0.5]),
            v(1., 1., 0., 1., 1., 1., [1., 0., 0., 0.5]),
            v(-1., 1., 0., 1., 0., 1., [1., 0., 0., 0.5]),
        ];
        frame
            .draw_indexed(&s, None, &vertices, &[0, 1, 2, 0, 2, 3])
            .unwrap();
        // Every covered pixel is touched once: 0.5 red, never the 0.75 a
        // duplicated shared edge would produce.
        assert!(frame.rgba.chunks_exact(4).all(|pixel| pixel[0] == 128));
    }
}
