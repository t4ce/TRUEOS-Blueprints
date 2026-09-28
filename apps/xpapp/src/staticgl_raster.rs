//! Small, deterministic fixed-function raster core used by the XPAPP OpenGL
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DrawTiming {
    pub total: std::time::Duration,
    pub prepare: std::time::Duration,
    pub capacity: std::time::Duration,
    pub copy_in: std::time::Duration,
    pub copy_out: std::time::Duration,
    pub scalar: std::time::Duration,
    pub texture_bytes: u64,
    pub framebuffer_bytes: u64,
    pub parallel: bool,
    pub pool: crate::staticgl_raster_pool::PoolTiming,
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub width: u32,
    /// Full framebuffer height used for GL window coordinates.
    pub height: u32,
    /// Last global top-down row stored in this allocation.  The normal frame
    /// owns every row; a worker band owns only an inclusive subrange.
    storage_bottom: i32,
    pub rgba: Vec<u8>,
    pub depth: Vec<f32>,
    pub timing: DrawTiming,
    // Draw preparation buffers retain their capacity across scalar draws.
    // Band frames start empty and never use them.
    clip_codes: Vec<u8>,
    staged: Vec<[ClipVertex; 3]>,
    clip_polygon: Vec<ClipVertex>,
    clip_scratch: Vec<ClipVertex>,
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
    Worker,
}

pub(crate) const MAX_RASTER_PIXELS: usize = 1920 * 1080 * 2;
const RASTER_ROWS_PER_STEP: i32 = 8;

#[cfg(not(test))]
fn report_worker_mode(parallel: bool) {
    use core::sync::atomic::{AtomicU8, Ordering};
    static REPORTED: AtomicU8 = AtomicU8::new(0);
    let mode = if parallel { 1 } else { 2 };
    if REPORTED.swap(mode, Ordering::AcqRel) != mode {
        let description = if parallel {
            "two persistent P-core row-band workers"
        } else {
            "scalar (pool disabled or fewer than two P-core workers)"
        };
        crate::logl::emit(
            crate::logl::level::IMPORTANT,
            format_args!("XPAPP CPU RASTER mode={description}"),
        );
    }
}

#[cfg(test)]
fn report_worker_mode(_: bool) {}

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
            storage_bottom: height as i32 - 1,
            rgba: vec![0; pixels.checked_mul(4).ok_or(RasterError::BadExtent)?],
            depth: vec![1.0; pixels],
            timing: DrawTiming::default(),
            clip_codes: Vec::new(),
            staged: Vec::new(),
            clip_polygon: Vec::new(),
            clip_scratch: Vec::new(),
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
        if color.is_none() && depth.is_none() {
            return;
        }
        let Some([left, right, bottom, top]) = clear_bounds(self.width, self.height, scissor)
        else {
            return;
        };
        let row_width = self.width as usize;
        for y in bottom..top {
            let start = y as usize * row_width + left as usize;
            let end = y as usize * row_width + right as usize;
            if let Some(color) = color {
                for pixel in self.rgba[start * 4..end * 4].chunks_exact_mut(4) {
                    pixel.copy_from_slice(&color);
                }
            }
            if let Some(depth) = depth {
                self.depth[start..end].fill(depth);
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
        let started = std::time::Instant::now();
        self.timing = DrawTiming::default();
        self.clip_codes.clear();
        self.staged.clear();
        self.clip_polygon.clear();
        self.clip_scratch.clear();
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
        // Classify each source vertex once.  Almost all XPAPP geometry is fully
        // inside the canonical volume, so avoid allocating a polygon and
        // running all seven clipping passes for every such triangle.
        self.clip_codes
            .extend(vertices.iter().copied().map(clip_code));
        self.staged.reserve(indices.len() / 3);
        for tri in indices.chunks_exact(3) {
            let codes = [
                self.clip_codes[tri[0] as usize],
                self.clip_codes[tri[1] as usize],
                self.clip_codes[tri[2] as usize],
            ];
            let any_outside = codes[0] | codes[1] | codes[2];
            if any_outside == 0 {
                self.staged.push([
                    vertices[tri[0] as usize],
                    vertices[tri[1] as usize],
                    vertices[tri[2] as usize],
                ]);
                continue;
            }
            if codes[0] & codes[1] & codes[2] != 0 {
                continue;
            }
            clip_triangle(
                [
                    vertices[tri[0] as usize],
                    vertices[tri[1] as usize],
                    vertices[tri[2] as usize],
                ],
                &mut self.clip_polygon,
                &mut self.clip_scratch,
            );
            for n in 1..self.clip_polygon.len().saturating_sub(1) {
                self.staged.push([
                    self.clip_polygon[0],
                    self.clip_polygon[n],
                    self.clip_polygon[n + 1],
                ]);
            }
        }
        let mut stats = RasterStats {
            input_triangles: (indices.len() / 3) as u32,
            clipped_triangles: self.staged.len() as u32,
            shaded_pixels: 0,
        };
        self.timing.prepare = started.elapsed();
        let capacity_started = std::time::Instant::now();
        let parallel = crate::staticgl_raster_pool::has_two_workers();
        self.timing.capacity = capacity_started.elapsed();
        self.timing.parallel = parallel;
        if parallel {
            report_worker_mode(true);
            // The experimental pool owns its triangle slice through the job
            // lifetime. Keep that ownership path unchanged; scalar draws
            // retain and reuse the preparation buffer below.
            let staged = core::mem::take(&mut self.staged);
            stats.shaded_pixels = self
                .draw_staged_banded_pool(*state, texture, staged)
                .map_err(|_| RasterError::Worker)? as u64;
        } else {
            report_worker_mode(false);
            let scalar_started = std::time::Instant::now();
            let mut staged = core::mem::take(&mut self.staged);
            for tri in staged.drain(..) {
                stats.shaded_pixels += self.draw_triangle(state, texture, tri) as u64;
            }
            self.staged = staged;
            self.timing.scalar = scalar_started.elapsed();
        }
        self.timing.total = started.elapsed();
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
        self.draw_triangle_rows(state, texture, tri, i32::MIN, i32::MAX)
    }

    /// Rasterizes an inclusive top-down row interval of a triangle.  The
    /// affine row values always start at the triangle's first visible row and
    /// advance with the scalar `+= dy` recurrence before this interval begins.
    /// That is deliberately different from evaluating a plane at `row_start`:
    /// it keeps the same f32 rounding as the unsplit renderer.
    fn draw_triangle_rows(
        &mut self,
        state: &RasterState,
        texture: Option<TextureView<'_>>,
        tri: [ClipVertex; 3],
        row_start: i32,
        row_end: i32,
    ) -> usize {
        let mut p = tri.map(|v| ScreenVertex::from_clip(v, state.viewport, self.height));
        let area_gl = orient([p[0].x, -p[0].y], [p[1].x, -p[1].y], [p[2].x, -p[2].y]);
        // Source vertices are finite, but an extreme finite clip coordinate
        // can still overflow during the perspective divide.  Such a primitive
        // has no meaningful finite coverage; rejecting it keeps NaNs out of
        // the incremental edge and attribute planes below.
        if !area_gl.is_finite() || area_gl == 0.0 {
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
        if !area.is_finite() || area == 0.0 {
            return 0;
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
        let mut min_x = p
            .iter()
            .map(|v| v.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as i32;
        let mut max_x = p
            .iter()
            .map(|v| v.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width as f32 - 1.0) as i32;
        let mut min_y = p
            .iter()
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as i32;
        let mut max_y = p
            .iter()
            .map(|v| v.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height as f32 - 1.0) as i32;
        if state.scissor_enabled {
            let Some([left, right, top, bottom]) = scissor_bounds(self.height, state.scissor)
            else {
                return 0;
            };
            min_x = min_x.max(left);
            max_x = max_x.min(right);
            min_y = min_y.max(top);
            max_y = max_y.min(bottom);
        }
        if min_x > max_x || min_y > max_y {
            return 0;
        }
        let render_min_y = min_y.max(row_start);
        let render_max_y = max_y.min(row_end);
        if render_min_y > render_max_y {
            return 0;
        }

        // This is the dominant menu/background state: an opaque RGBA texture
        // replacing the primary colour.  It has no observable dependency on
        // colour, depth, fog, alpha test, or blending, so keep those planes and
        // branches out of its inner loop altogether.
        if texture.is_some()
            && !state.depth.enabled
            && !state.alpha.enabled
            && !state.blend.enabled
            && !state.fog.enabled
            && state.tex_env == TexEnvMode::Replace
            && texture.unwrap().format == TextureFormat::Rgba
        {
            return self.draw_opaque_rgba_replace(
                texture.unwrap(),
                p,
                min_x,
                max_x,
                min_y,
                render_min_y,
                render_max_y,
                area,
            );
        }

        // Edge functions and every interpolated value are affine in window
        // coordinates.  Calculate their values at the first pixel centre once
        // and then advance them across scanlines.  The former implementation
        // rebuilt three barycentric coordinates (and repeated perspective
        // reconstruction) for every covered pixel; XPAPP's full-screen menu
        // geometry made that the overwhelmingly dominant CPU cost.
        let edges = [
            Edge::new(p[1], p[2]),
            Edge::new(p[2], p[0]),
            Edge::new(p[0], p[1]),
        ];
        let start = [min_x as f32 + 0.5, min_y as f32 + 0.5];
        let edge_start = edges.map(|edge| edge.at(start));
        let inv_area = area.recip();
        let depth_plane = Plane::from_vertices(
            edge_start,
            &edges,
            inv_area,
            [p[0].z_ndc, p[1].z_ndc, p[2].z_ndc],
        );
        let inv_w_plane = Plane::from_vertices(
            edge_start,
            &edges,
            inv_area,
            [p[0].inv_w, p[1].inv_w, p[2].inv_w],
        );
        let color_planes: [Plane; 4] = core::array::from_fn(|i| {
            Plane::from_vertices(
                edge_start,
                &edges,
                inv_area,
                [
                    p[0].color_over_w[i],
                    p[1].color_over_w[i],
                    p[2].color_over_w[i],
                ],
            )
        });
        let uv_planes: [Plane; 4] = core::array::from_fn(|i| {
            Plane::from_vertices(
                edge_start,
                &edges,
                inv_area,
                [p[0].uv_over_w[i], p[1].uv_over_w[i], p[2].uv_over_w[i]],
            )
        });
        let fog_plane = Plane::from_vertices(
            edge_start,
            &edges,
            inv_area,
            [p[0].fog_over_w, p[1].fog_over_w, p[2].fog_over_w],
        );
        let edge_dx = edges.map(|edge| edge.dx);
        let edge_dy = edges.map(|edge| edge.dy);
        let top_left = edges.map(|edge| edge.top_left);
        let depth_scale = state.depth.range[1] - state.depth.range[0];
        let mut shaded = 0;
        let mut edge_row = edge_start;
        let mut depth_row = depth_plane.value;
        let mut inv_w_row = inv_w_plane.value;
        let mut color_row = color_planes.map(|plane| plane.value);
        let mut uv_row = uv_planes.map(|plane| plane.value);
        let mut fog_row = fog_plane.value;
        for _ in min_y..render_min_y {
            for i in 0..3 {
                edge_row[i] += edge_dy[i];
            }
            depth_row += depth_plane.dy;
            inv_w_row += inv_w_plane.dy;
            for i in 0..4 {
                color_row[i] += color_planes[i].dy;
                uv_row[i] += uv_planes[i].dy;
            }
            fog_row += fog_plane.dy;
        }
        for y in render_min_y..=render_max_y {
            let mut edge = edge_row;
            let mut z_ndc = depth_row;
            let mut inv_w = inv_w_row;
            let mut color_over_w = color_row;
            let mut uv_over_w = uv_row;
            let mut fog_over_w = fog_row;
            let mut offset = self.top_index(min_x, y);
            for _x in min_x..=max_x {
                if edge_inside_fast(edge[0], top_left[0])
                    && edge_inside_fast(edge[1], top_left[1])
                    && edge_inside_fast(edge[2], top_left[2])
                {
                    let depth =
                        (state.depth.range[0] + (z_ndc * 0.5 + 0.5) * depth_scale + polygon_bias)
                            .clamp(0.0, 1.0);
                    if !state.depth.enabled || compare(state.depth.func, depth, self.depth[offset])
                    {
                        // Perspective attributes share one reciprocal.  Texture
                        // s/t further cancel that reciprocal against q, so the
                        // sampler only needs the q-over-w planes.
                        let reciprocal_w = inv_w.recip();
                        let mut color = color_over_w.map(|value| value * reciprocal_w);
                        if let Some(texture) = texture {
                            let q = uv_over_w[3];
                            let sample = sample_texture_uv(
                                texture,
                                uv_over_w[0] / q,
                                uv_over_w[1] / q,
                                (uv_over_w[0] + uv_planes[0].dx) / (q + uv_planes[3].dx),
                                (uv_over_w[1] + uv_planes[1].dx) / (q + uv_planes[3].dx),
                                (uv_over_w[0] + uv_planes[0].dy) / (q + uv_planes[3].dy),
                                (uv_over_w[1] + uv_planes[1].dy) / (q + uv_planes[3].dy),
                            );
                            color = texenv(
                                state.tex_env,
                                state.tex_env_color,
                                texture.format,
                                color,
                                sample,
                            );
                        }
                        if state.fog.enabled {
                            color = apply_fog(state.fog, fog_over_w * reciprocal_w, color);
                        }
                        color = color.map(clamp01);
                        if !state.alpha.enabled
                            || compare(state.alpha.func, color[3], state.alpha.reference)
                        {
                            if state.blend.enabled {
                                color = blend(state.blend, color, read_pixel(&self.rgba, offset));
                            }
                            write_pixel(&mut self.rgba, offset, color);
                            if state.depth.enabled && state.depth.write {
                                self.depth[offset] = depth;
                            }
                            shaded += 1;
                        }
                    }
                }
                edge[0] += edge_dx[0];
                edge[1] += edge_dx[1];
                edge[2] += edge_dx[2];
                z_ndc += depth_plane.dx;
                inv_w += inv_w_plane.dx;
                for i in 0..4 {
                    color_over_w[i] += color_planes[i].dx;
                    uv_over_w[i] += uv_planes[i].dx;
                }
                fog_over_w += fog_plane.dx;
                offset += 1;
            }
            for i in 0..3 {
                edge_row[i] += edge_dy[i];
            }
            depth_row += depth_plane.dy;
            inv_w_row += inv_w_plane.dy;
            for i in 0..4 {
                color_row[i] += color_planes[i].dy;
                uv_row[i] += uv_planes[i].dy;
            }
            fog_row += fog_plane.dy;
        }
        shaded
    }

    #[inline(never)]
    fn draw_opaque_rgba_replace(
        &mut self,
        texture: TextureView<'_>,
        p: [ScreenVertex; 3],
        min_x: i32,
        max_x: i32,
        min_y: i32,
        render_min_y: i32,
        render_max_y: i32,
        area: f32,
    ) -> usize {
        let edges = [
            Edge::new(p[1], p[2]),
            Edge::new(p[2], p[0]),
            Edge::new(p[0], p[1]),
        ];
        let edge_dx = edges.map(|edge| edge.dx);
        let edge_dy = edges.map(|edge| edge.dy);
        let top_left = edges.map(|edge| edge.top_left);
        let start = [min_x as f32 + 0.5, min_y as f32 + 0.5];
        let edge_start = edges.map(|edge| edge.at(start));
        let inv_area = area.recip();
        let uv_planes: [Plane; 4] = core::array::from_fn(|i| {
            Plane::from_vertices(
                edge_start,
                &edges,
                inv_area,
                [p[0].uv_over_w[i], p[1].uv_over_w[i], p[2].uv_over_w[i]],
            )
        });
        let mut shaded = 0;
        let mut edge_row = edge_start;
        let mut uv_row = uv_planes.map(|plane| plane.value);
        for _ in min_y..render_min_y {
            for i in 0..3 {
                edge_row[i] += edge_dy[i];
            }
            for i in 0..4 {
                uv_row[i] += uv_planes[i].dy;
            }
        }

        // UI quads normally have both clip W and texture Q equal to one.  Q is
        // then a constant plane, allowing s/t and their gradients to use two
        // multiplies rather than six per-pixel divisions.
        if uv_planes[3].dx == 0.0
            && uv_planes[3].dy == 0.0
            && uv_planes[3].value.is_finite()
            && uv_planes[3].value != 0.0
        {
            let inv_q = uv_planes[3].value.recip();
            let u_dx = uv_planes[0].dx * inv_q;
            let v_dx = uv_planes[1].dx * inv_q;
            let u_dy = uv_planes[0].dy * inv_q;
            let v_dy = uv_planes[1].dy * inv_q;
            let sample_plan = texture_sample_plan(texture, u_dx, v_dx, u_dy, v_dy);
            for y in render_min_y..=render_max_y {
                let mut edge = edge_row;
                let mut uv = uv_row;
                let mut offset = self.top_index(min_x, y);
                for _x in min_x..=max_x {
                    if edge_inside_fast(edge[0], top_left[0])
                        && edge_inside_fast(edge[1], top_left[1])
                        && edge_inside_fast(edge[2], top_left[2])
                    {
                        write_pixel(
                            &mut self.rgba,
                            offset,
                            sample_texture_plan(sample_plan, uv[0] * inv_q, uv[1] * inv_q),
                        );
                        shaded += 1;
                    }
                    edge[0] += edge_dx[0];
                    edge[1] += edge_dx[1];
                    edge[2] += edge_dx[2];
                    for i in 0..4 {
                        uv[i] += uv_planes[i].dx;
                    }
                    offset += 1;
                }
                for i in 0..3 {
                    edge_row[i] += edge_dy[i];
                }
                for i in 0..4 {
                    uv_row[i] += uv_planes[i].dy;
                }
            }
        } else {
            for y in render_min_y..=render_max_y {
                let mut edge = edge_row;
                let mut uv = uv_row;
                let mut offset = self.top_index(min_x, y);
                for _x in min_x..=max_x {
                    if edge_inside_fast(edge[0], top_left[0])
                        && edge_inside_fast(edge[1], top_left[1])
                        && edge_inside_fast(edge[2], top_left[2])
                    {
                        let q = uv[3];
                        write_pixel(
                            &mut self.rgba,
                            offset,
                            sample_texture_uv(
                                texture,
                                uv[0] / q,
                                uv[1] / q,
                                (uv[0] + uv_planes[0].dx) / (q + uv_planes[3].dx),
                                (uv[1] + uv_planes[1].dx) / (q + uv_planes[3].dx),
                                (uv[0] + uv_planes[0].dy) / (q + uv_planes[3].dy),
                                (uv[1] + uv_planes[1].dy) / (q + uv_planes[3].dy),
                            ),
                        );
                        shaded += 1;
                    }
                    edge[0] += edge_dx[0];
                    edge[1] += edge_dx[1];
                    edge[2] += edge_dx[2];
                    for i in 0..4 {
                        uv[i] += uv_planes[i].dx;
                    }
                    offset += 1;
                }
                for i in 0..3 {
                    edge_row[i] += edge_dy[i];
                }
                for i in 0..4 {
                    uv_row[i] += uv_planes[i].dy;
                }
            }
        }
        shaded
    }

    /// Converts the raster's top-down coverage coordinate into its bottom-up
    /// GL backing row.
    fn top_index(&self, x: i32, y_top: i32) -> usize {
        debug_assert!(y_top <= self.storage_bottom);
        (self.storage_bottom as usize - y_top as usize) * self.width as usize + x as usize
    }

    fn copy_rows(&self, top: i32, bottom: i32) -> Frame {
        let rows = (bottom - top + 1) as usize;
        let width = self.width as usize;
        let mut rgba = vec![0; rows * width * 4];
        let mut depth = vec![0.0; rows * width];
        for y in top..=bottom {
            let source = self.top_index(0, y);
            let destination = (bottom - y) as usize * width;
            rgba[destination * 4..(destination + width) * 4]
                .copy_from_slice(&self.rgba[source * 4..(source + width) * 4]);
            depth[destination..destination + width]
                .copy_from_slice(&self.depth[source..source + width]);
        }
        Frame {
            width: self.width,
            height: self.height,
            storage_bottom: bottom,
            rgba,
            depth,
            timing: DrawTiming::default(),
            clip_codes: Vec::new(),
            staged: Vec::new(),
            clip_polygon: Vec::new(),
            clip_scratch: Vec::new(),
        }
    }

    fn copy_band_back(&mut self, band: &RasterBandJob) {
        let width = self.width as usize;
        for y in band.band_start..=band.band_end {
            let destination = self.top_index(0, y);
            let source = band.frame.top_index(0, y);
            self.rgba[destination * 4..(destination + width) * 4]
                .copy_from_slice(&band.frame.rgba[source * 4..(source + width) * 4]);
            self.depth[destination..destination + width]
                .copy_from_slice(&band.frame.depth[source..source + width]);
        }
    }

    fn draw_staged_banded_pool(
        &mut self,
        state: RasterState,
        texture: Option<TextureView<'_>>,
        triangles: Vec<[ClipVertex; 3]>,
    ) -> Result<usize, trueos::worker::SpawnError> {
        let copy_started = std::time::Instant::now();
        self.timing.texture_bytes = texture.map_or(0, |t| t.levels.iter().map(|l| l.rgba.len() as u64).sum());
        self.timing.framebuffer_bytes = u64::from(self.width) * u64::from(self.height) * 8 * 2;
        let triangles: std::sync::Arc<[[ClipVertex; 3]]> = triangles.into();
        let texture = texture.map(OwnedTexture::copy_of).map(std::sync::Arc::new);
        let split = (self.height as i32 + 1) / 2;
        let upper = RasterBandJob::new(
            self.copy_rows(0, split - 1),
            state,
            texture.clone(),
            triangles.clone(),
            0,
            split - 1,
        );
        let lower = RasterBandJob::new(
            self.copy_rows(split, self.height as i32 - 1),
            state,
            texture,
            triangles,
            split,
            self.height as i32 - 1,
        );
        self.timing.copy_in = copy_started.elapsed();
        let (upper, lower, pool) =
            crate::staticgl_raster_pool::run_two(upper, lower, RasterBandJob::step)?;

        // No parent framebuffer row is replaced before both worker handles
        // complete.  This keeps a failed/tearing-down draw from becoming a
        // partial visible frame.
        let shaded = upper.shaded + lower.shaded;
        self.timing.pool = pool;
        let copy_started = std::time::Instant::now();
        self.copy_band_back(&upper);
        self.copy_band_back(&lower);
        self.timing.copy_out = copy_started.elapsed();
        Ok(shaded)
    }

    /// Executes the same two owned screen bands without native workers.  This
    /// is used by exact host parity tests; production submits these jobs to
    /// the persistent P-core pool and joins them before the draw returns.
    #[cfg(test)]
    fn draw_staged_banded_scalar(
        &mut self,
        state: RasterState,
        texture: Option<TextureView<'_>>,
        triangles: Vec<[ClipVertex; 3]>,
    ) -> usize {
        let triangles: std::sync::Arc<[[ClipVertex; 3]]> = triangles.into();
        let texture = texture.map(OwnedTexture::copy_of).map(std::sync::Arc::new);
        let split = (self.height as i32 + 1) / 2;
        let mut upper = RasterBandJob::new(
            self.copy_rows(0, split - 1),
            state,
            texture.clone(),
            triangles.clone(),
            0,
            split - 1,
        );
        let mut lower = RasterBandJob::new(
            self.copy_rows(split, self.height as i32 - 1),
            state,
            texture,
            triangles,
            split,
            self.height as i32 - 1,
        );
        loop {
            let upper_more = upper.step();
            let lower_more = lower.step();
            if !upper_more && !lower_more {
                break;
            }
        }
        let shaded = upper.shaded + lower.shaded;
        self.copy_band_back(&upper);
        self.copy_band_back(&lower);
        shaded
    }
}

struct RasterBandJob {
    frame: Frame,
    state: RasterState,
    texture: Option<std::sync::Arc<OwnedTexture>>,
    triangles: std::sync::Arc<[[ClipVertex; 3]]>,
    band_start: i32,
    band_end: i32,
    triangle: usize,
    next_row: i32,
    shaded: usize,
}

impl RasterBandJob {
    fn new(
        frame: Frame,
        state: RasterState,
        texture: Option<std::sync::Arc<OwnedTexture>>,
        triangles: std::sync::Arc<[[ClipVertex; 3]]>,
        band_start: i32,
        band_end: i32,
    ) -> Self {
        Self {
            frame,
            state,
            texture,
            triangles,
            band_start,
            band_end,
            triangle: 0,
            next_row: band_start,
            shaded: 0,
        }
    }

    /// Executes at most eight rows of the current triangle.  The persistent
    /// worker requeues this same job after its four-millisecond budget; no
    /// partially rendered triangle is observed outside this draw call.
    fn step(&mut self) -> bool {
        let Some(&triangle) = self.triangles.get(self.triangle) else {
            return false;
        };
        let end = self
            .next_row
            .saturating_add(RASTER_ROWS_PER_STEP - 1)
            .min(self.band_end);
        self.shaded += match &self.texture {
            Some(texture) => texture.with_view(|view| {
                self.frame
                    .draw_triangle_rows(&self.state, Some(view), triangle, self.next_row, end)
            }),
            None => self
                .frame
                .draw_triangle_rows(&self.state, None, triangle, self.next_row, end),
        };
        self.next_row = end.saturating_add(1);
        if self.next_row > self.band_end {
            self.triangle += 1;
            self.next_row = self.band_start;
        }
        self.triangle < self.triangles.len()
    }
}

/// Fully owned texture storage shared by the two worker jobs.  Pixel data is
/// copied once when a draw enters the P-core path, so queued work never holds
/// references into a mutable GL context or decoded temporary vectors.
struct OwnedTexture {
    levels: Vec<OwnedTextureLevel>,
    format: TextureFormat,
    wrap_s: Wrap,
    wrap_t: Wrap,
    min_filter: Filter,
    mag_filter: Filter,
}

struct OwnedTextureLevel {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl OwnedTexture {
    fn copy_of(texture: TextureView<'_>) -> Self {
        Self {
            levels: texture
                .levels
                .iter()
                .map(|level| OwnedTextureLevel {
                    width: level.width,
                    height: level.height,
                    rgba: level.rgba.to_vec(),
                })
                .collect(),
            format: texture.format,
            wrap_s: texture.wrap_s,
            wrap_t: texture.wrap_t,
            min_filter: texture.min_filter,
            mag_filter: texture.mag_filter,
        }
    }

    fn with_view<R>(&self, f: impl FnOnce(TextureView<'_>) -> R) -> R {
        const STACK_LEVELS: usize = 32;
        if self.levels.len() <= STACK_LEVELS {
            // Mip chains have at most twelve levels within XPAPP's raster
            // pixel budget. Keep their borrowed descriptors on the worker
            // stack so each bounded row step avoids allocator traffic.
            let mut levels = [TextureLevel {
                width: 0,
                height: 0,
                rgba: &[],
            }; STACK_LEVELS];
            for (descriptor, level) in levels.iter_mut().zip(&self.levels) {
                *descriptor = TextureLevel {
                    width: level.width,
                    height: level.height,
                    rgba: &level.rgba,
                };
            }
            f(TextureView {
                levels: &levels[..self.levels.len()],
                format: self.format,
                wrap_s: self.wrap_s,
                wrap_t: self.wrap_t,
                min_filter: self.min_filter,
                mag_filter: self.mag_filter,
            })
        } else {
            // Retain the general GL contract for unusually deep supplied
            // chains without burdening the bounded normal path.
            let levels: Vec<_> = self
                .levels
                .iter()
                .map(|level| TextureLevel {
                    width: level.width,
                    height: level.height,
                    rgba: &level.rgba,
                })
                .collect();
            f(TextureView {
                levels: &levels,
                format: self.format,
                wrap_s: self.wrap_s,
                wrap_t: self.wrap_t,
                min_filter: self.min_filter,
                mag_filter: self.mag_filter,
            })
        }
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

#[derive(Clone, Copy)]
struct Edge {
    dx: f32,
    dy: f32,
    top_left: bool,
    c: f32,
}

impl Edge {
    #[inline]
    fn new(a: ScreenVertex, b: ScreenVertex) -> Self {
        // orient(a, b, [x, y]) = dx * x + dy * y + c.
        Self {
            dx: a.y - b.y,
            dy: b.x - a.x,
            top_left: (b.y - a.y) < 0.0 || ((b.y - a.y) == 0.0 && (b.x - a.x) > 0.0),
            c: b.y * a.x - b.x * a.y,
        }
    }

    #[inline]
    fn at(self, point: [f32; 2]) -> f32 {
        self.dx * point[0] + self.dy * point[1] + self.c
    }
}

#[derive(Clone, Copy)]
struct Plane {
    value: f32,
    dx: f32,
    dy: f32,
}

impl Plane {
    #[inline]
    fn from_vertices(edge: [f32; 3], edges: &[Edge; 3], inv_area: f32, values: [f32; 3]) -> Self {
        Self {
            value: (edge[0] * values[0] + edge[1] * values[1] + edge[2] * values[2]) * inv_area,
            dx: (edges[0].dx * values[0] + edges[1].dx * values[1] + edges[2].dx * values[2])
                * inv_area,
            dy: (edges[0].dy * values[0] + edges[1].dy * values[1] + edges[2].dy * values[2])
                * inv_area,
        }
    }
}

#[inline]
fn edge_inside_fast(edge: f32, top_left: bool) -> bool {
    edge > 0.0 || (edge == 0.0 && top_left)
}

/// Returns an inclusive top-down rectangle, clipped to the framebuffer.
fn scissor_bounds(frame_height: u32, scissor: [i32; 4]) -> Option<[i32; 4]> {
    let [left, bottom, width, height] = scissor.map(i64::from);
    if width <= 0 || height <= 0 {
        return None;
    }
    let frame_height = i64::from(frame_height);
    let right = left.saturating_add(width).saturating_sub(1);
    let top = frame_height.saturating_sub(bottom.saturating_add(height));
    let bottom_top = frame_height.saturating_sub(bottom).saturating_sub(1);
    let left = left.max(0);
    let right = right.min(i64::from(i32::MAX));
    let top = top.max(0);
    let bottom_top = bottom_top.min(frame_height.saturating_sub(1));
    if left > right || top > bottom_top {
        return None;
    }
    Some([left as i32, right as i32, top as i32, bottom_top as i32])
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
/// Returns an exclusive lower-left rectangle clipped to the framebuffer.
fn clear_bounds(
    frame_width: u32,
    frame_height: u32,
    scissor: Option<[i32; 4]>,
) -> Option<[i32; 4]> {
    let width = i64::from(frame_width);
    let height = i64::from(frame_height);
    let [left, right, bottom, top] = match scissor {
        None => [0, width, 0, height],
        Some(scissor) => {
            let [left, bottom, scissor_width, scissor_height] = scissor.map(i64::from);
            if scissor_width <= 0 || scissor_height <= 0 {
                return None;
            }
            [
                left.max(0),
                left.saturating_add(scissor_width).min(width),
                bottom.max(0),
                bottom.saturating_add(scissor_height).min(height),
            ]
        }
    };
    if left >= right || bottom >= top {
        return None;
    }
    Some([left as i32, right as i32, bottom as i32, top as i32])
}
fn vertex_valid(v: ClipVertex) -> bool {
    v.clip
        .iter()
        .chain(v.color.iter())
        .chain(v.uv.iter())
        .chain([v.fog].iter())
        .all(|x| x.is_finite())
}
#[inline]
fn clip_code(v: ClipVertex) -> u8 {
    let mut code = 0;
    for plane in 0..7 {
        if plane_distance(v.clip, plane) < 0.0 {
            code |= 1 << plane;
        }
    }
    code
}
fn clip_triangle(
    input: [ClipVertex; 3],
    polygon: &mut Vec<ClipVertex>,
    scratch: &mut Vec<ClipVertex>,
) {
    polygon.clear();
    polygon.extend_from_slice(&input);
    scratch.clear();
    // The six canonical clip half-spaces imply w >= 0.  A point exactly at
    // the homogeneous origin satisfies them all but cannot be divided.  Clip
    // it to a small positive-w plane before the perspective divide; this is
    // a finite representation of the same limiting primitive.
    for plane in 0..7 {
        if polygon.is_empty() {
            break;
        }
        scratch.clear();
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let da = plane_distance(a.clip, plane);
            let db = plane_distance(b.clip, plane);
            let ina = da >= 0.0;
            let inb = db >= 0.0;
            if ina {
                scratch.push(a);
            }
            if ina != inb {
                scratch.push(lerp_vertex(a, b, da / (da - db)));
            }
        }
        core::mem::swap(polygon, scratch);
    }
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
fn perspective_weight(p: &[ScreenVertex; 3], b: [f32; 3]) -> [f32; 3] {
    let d = b[0] * p[0].inv_w + b[1] * p[1].inv_w + b[2] * p[2].inv_w;
    [
        b[0] * p[0].inv_w / d,
        b[1] * p[1].inv_w / d,
        b[2] * p[2].inv_w / d,
    ]
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
/// Samples a texture from perspective-correct s/t and their one-pixel
/// derivatives.  The rasterizer has already interpolated q-over-w, so it can
/// form these values without reconstructing barycentrics at the neighbouring
/// pixel centres.
#[derive(Clone, Copy)]
enum TextureSamplePlan<'a> {
    Single {
        level: TextureLevel<'a>,
        wrap_s: Wrap,
        wrap_t: Wrap,
        linear: bool,
    },
    Lerp {
        a: TextureLevel<'a>,
        b: TextureLevel<'a>,
        wrap_s: Wrap,
        wrap_t: Wrap,
        linear: bool,
        t: f32,
    },
}

/// Selects the filter and mip levels once when an affine texture mapping has
/// constant pixel derivatives.  This is valid for the Q-constant UI path and
/// removes a pair of hypot calls and log2 from every fragment.
fn texture_sample_plan(
    tex: TextureView<'_>,
    u_dx: f32,
    v_dx: f32,
    u_dy: f32,
    v_dy: f32,
) -> TextureSamplePlan<'_> {
    let base = &tex.levels[0];
    let lod = match (tex.min_filter, tex.mag_filter) {
        (Filter::Nearest, Filter::Nearest) | (Filter::Linear, Filter::Linear) => 0.0,
        _ => {
            let du = (u_dx * base.width as f32).hypot(u_dy * base.width as f32);
            let dv = (v_dx * base.height as f32).hypot(v_dy * base.height as f32);
            du.max(dv).max(1.0).log2()
        }
    };
    let filter = if lod <= 0.0 {
        tex.mag_filter
    } else {
        tex.min_filter
    };
    let base_plan = |level, linear| TextureSamplePlan::Single {
        level,
        wrap_s: tex.wrap_s,
        wrap_t: tex.wrap_t,
        linear,
    };
    match filter {
        Filter::Nearest => base_plan(*base, false),
        Filter::Linear => base_plan(*base, true),
        Filter::NearestMipmapNearest | Filter::LinearMipmapNearest => {
            let level = (lod.round().max(0.0) as usize).min(tex.levels.len() - 1);
            base_plan(tex.levels[level], filter == Filter::LinearMipmapNearest)
        }
        Filter::NearestMipmapLinear | Filter::LinearMipmapLinear => {
            let a = lod.floor().max(0.0) as usize;
            let a = a.min(tex.levels.len() - 1);
            let b = (a + 1).min(tex.levels.len() - 1);
            TextureSamplePlan::Lerp {
                a: tex.levels[a],
                b: tex.levels[b],
                wrap_s: tex.wrap_s,
                wrap_t: tex.wrap_t,
                linear: filter == Filter::LinearMipmapLinear,
                t: lod - a as f32,
            }
        }
    }
}

#[inline]
fn sample_texture_plan(plan: TextureSamplePlan<'_>, u: f32, v: f32) -> [f32; 4] {
    let uv = [u, v, 0.0, 1.0];
    match plan {
        TextureSamplePlan::Single {
            level,
            wrap_s,
            wrap_t,
            linear,
        } => sample_level(level, uv, wrap_s, wrap_t, linear),
        TextureSamplePlan::Lerp {
            a,
            b,
            wrap_s,
            wrap_t,
            linear,
            t,
        } => lerp4(
            sample_level(a, uv, wrap_s, wrap_t, linear),
            sample_level(b, uv, wrap_s, wrap_t, linear),
            t,
        ),
    }
}

fn sample_texture_uv(
    tex: TextureView<'_>,
    u: f32,
    v: f32,
    u_dx: f32,
    v_dx: f32,
    u_dy: f32,
    v_dy: f32,
) -> [f32; 4] {
    let base = &tex.levels[0];
    let uv = [u, v, 0.0, 1.0];
    // Most UI textures use the same non-mipmap filter for magnification and
    // minification.  In that case GL's LOD has no observable effect, so avoid
    // two hypot calls and log2 for every fragment.
    match (tex.min_filter, tex.mag_filter) {
        (Filter::Nearest, Filter::Nearest) => {
            return sample_level(*base, uv, tex.wrap_s, tex.wrap_t, false);
        }
        (Filter::Linear, Filter::Linear) => {
            return sample_level(*base, uv, tex.wrap_s, tex.wrap_t, true);
        }
        _ => {}
    }
    let du = ((u_dx - u) * base.width as f32).hypot((u_dy - u) * base.width as f32);
    let dv = ((v_dx - v) * base.height as f32).hypot((v_dy - v) * base.height as f32);
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
    // Nearest-mip filters select one level. Clamping can also collapse a
    // two-level blend at the end of the chain. Avoid fetching the same texels
    // again; retain the old non-finite interpolation behavior.
    if t == 0.0 || (a == b && t.is_finite()) {
        return ca;
    }
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
    // XPAPP UI assets are overwhelmingly power-of-two repeat textures.  Their
    // four bilinear taps previously paid eight signed Euclidean divisions per
    // fragment through `fetch`/`wrap_index`; masking is exactly equivalent for
    // a positive power-of-two extent.
    if ws == Wrap::Repeat
        && wt == Wrap::Repeat
        && level.width.is_power_of_two()
        && level.height.is_power_of_two()
    {
        return sample_level_repeat_pow2(level, uv[0], uv[1], linear);
    }
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

#[inline]
fn sample_level_repeat_pow2(level: TextureLevel<'_>, u: f32, v: f32, linear: bool) -> [f32; 4] {
    let x = (u - u.floor()) * level.width as f32 - 0.5;
    let y = (v - v.floor()) * level.height as f32 - 0.5;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x_mask = level.width as i32 - 1;
    let y_mask = level.height as i32 - 1;
    if !linear {
        return fetch_repeat_pow2(level, x.round() as i32 & x_mask, y.round() as i32 & y_mask);
    }
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let x0 = x0 & x_mask;
    let y0 = y0 & y_mask;
    let x1 = (x0 + 1) & x_mask;
    let y1 = (y0 + 1) & y_mask;
    let a = lerp4(
        fetch_repeat_pow2(level, x0, y0),
        fetch_repeat_pow2(level, x1, y0),
        fx,
    );
    let b = lerp4(
        fetch_repeat_pow2(level, x0, y1),
        fetch_repeat_pow2(level, x1, y1),
        fx,
    );
    lerp4(a, b, fy)
}

#[inline]
fn fetch_repeat_pow2(l: TextureLevel<'_>, x: i32, y: i32) -> [f32; 4] {
    let p = &l.rgba[(y as usize * l.width as usize + x as usize) * 4..][..4];
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
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
        RasterError::Worker => "raster worker unavailable",
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
    fn preparation_scratch_reuses_capacity_and_recovers_after_bad_draw() {
        let tri = [
            v(-2.0, -0.7, 0.0, 1.0, 0.0, 0.0, [1.0, 0.0, 0.0, 1.0]),
            v(0.8, -0.7, 0.0, 1.0, 1.0, 0.0, [0.0, 1.0, 0.0, 1.0]),
            v(0.0, 0.8, 0.0, 1.0, 0.5, 1.0, [0.0, 0.0, 1.0, 1.0]),
        ];
        let mut frame = Frame::new(8, 8).unwrap();
        frame
            .draw_indexed(&state(), None, &tri, &[0, 1, 2])
            .unwrap();
        let capacities = (
            frame.clip_codes.capacity(),
            frame.staged.capacity(),
            frame.clip_polygon.capacity(),
            frame.clip_scratch.capacity(),
        );
        assert!(capacities.0 >= tri.len());
        assert!(capacities.1 > 0);
        assert!(capacities.2 > 0);
        assert!(capacities.3 > 0);

        assert_eq!(
            frame.draw_indexed(&state(), None, &tri, &[0, 1, 3]),
            Err(RasterError::BadIndex)
        );
        frame.clear(Some([0; 4]), Some(1.0), None);
        frame
            .draw_indexed(&state(), None, &tri, &[0, 1, 2])
            .unwrap();
        assert_eq!(
            (
                frame.clip_codes.capacity(),
                frame.staged.capacity(),
                frame.clip_polygon.capacity(),
                frame.clip_scratch.capacity(),
            ),
            capacities
        );

        let mut fresh = Frame::new(8, 8).unwrap();
        fresh
            .draw_indexed(&state(), None, &tri, &[0, 1, 2])
            .unwrap();
        assert_eq!(frame.rgba, fresh.rgba);
        assert_eq!(frame.depth, fresh.depth);
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

    #[test]
    fn opaque_rgba_replace_kernel_matches_general_pipeline() {
        let pixels = [
            10, 20, 30, 255, 80, 90, 100, 255, 140, 150, 160, 255, 220, 230, 240, 255,
        ];
        let levels = [TextureLevel {
            width: 2,
            height: 2,
            rgba: &pixels,
        }];
        let texture = TextureView {
            levels: &levels,
            format: TextureFormat::Rgba,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
        };
        let mut vertices = [
            v(-1., -1., 0., 1., 0., 0., [1.; 4]),
            v(1., -1., 0., 1., 1., 0., [1.; 4]),
            v(1., 1., 0., 1., 1., 1., [1.; 4]),
            v(-1., 1., 0., 1., 0., 1., [1.; 4]),
        ];
        for vertex in &mut vertices {
            vertex.uv[3] = 3.0;
        }
        let indices = [0, 1, 2, 0, 2, 3];
        let replace = RasterState {
            viewport: [0, 0, 8, 8],
            tex_env: TexEnvMode::Replace,
            ..Default::default()
        };
        let general = RasterState {
            depth: DepthState {
                enabled: true,
                func: Compare::Always,
                write: false,
                range: [0.0, 1.0],
            },
            ..replace
        };
        let mut fast = Frame::new(8, 8).unwrap();
        let mut fallback = Frame::new(8, 8).unwrap();
        fast.draw_indexed(&replace, Some(texture), &vertices, &indices)
            .unwrap();
        fallback
            .draw_indexed(&general, Some(texture), &vertices, &indices)
            .unwrap();
        assert_eq!(fast.rgba, fallback.rgba);
    }

    #[test]
    fn owned_row_bands_match_scalar_rgba_and_depth_bits() {
        let width = 127;
        let height = 93;
        let pixels: Vec<u8> = (0..64)
            .flat_map(|i| {
                [
                    (i * 3) as u8,
                    255u8.wrapping_sub((i * 2) as u8),
                    (i * 5) as u8,
                    48u8.wrapping_add((i * 3) as u8),
                ]
            })
            .collect();
        let levels = [TextureLevel {
            width: 8,
            height: 8,
            rgba: &pixels,
        }];
        let texture = TextureView {
            levels: &levels,
            format: TextureFormat::Rgba,
            wrap_s: Wrap::MirroredRepeat,
            wrap_t: Wrap::ClampToEdge,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
        };
        let state = RasterState {
            viewport: [0, 0, width, height],
            scissor_enabled: true,
            scissor: [11, 9, 103, 76],
            depth: DepthState {
                enabled: true,
                func: Compare::Lequal,
                write: true,
                range: [0.03, 0.91],
            },
            alpha: AlphaState {
                enabled: true,
                func: Compare::Greater,
                reference: 0.18,
            },
            blend: BlendState {
                enabled: true,
                src: BlendFactor::SrcAlpha,
                dst: BlendFactor::OneMinusSrcAlpha,
            },
            fog: FogState {
                enabled: true,
                mode: FogMode::Linear,
                color: [0.1, 0.2, 0.3, 1.0],
                density: 0.4,
                start: 0.2,
                end: 3.4,
            },
            tex_env: TexEnvMode::Modulate,
            ..Default::default()
        };
        let vertices = [
            v(-1.0, -1.0, -0.7, 1.0, -0.8, 0.1, [1.0, 0.2, 0.1, 0.9]),
            v(1.0, -0.8, 0.4, 1.2, 2.1, 0.3, [0.2, 1.0, 0.3, 0.7]),
            v(0.7, 0.8, 0.1, 0.9, 1.6, 2.4, [0.3, 0.4, 1.0, 0.8]),
            v(-0.9, 0.8, -0.4, 1.1, -1.4, 1.8, [1.0, 0.7, 0.2, 0.6]),
        ];
        let triangles = vec![
            [vertices[0], vertices[1], vertices[2]],
            [vertices[0], vertices[2], vertices[3]],
        ];
        let mut scalar = Frame::new(width as u32, height as u32).unwrap();
        let mut banded = scalar.clone();
        let mut chunked = scalar.clone();
        scalar.clear(Some([15, 19, 31, 127]), Some(0.83), None);
        banded.clear(Some([15, 19, 31, 127]), Some(0.83), None);
        chunked.clear(Some([15, 19, 31, 127]), Some(0.83), None);
        let scalar_stats = scalar
            .draw_indexed(&state, Some(texture), &vertices, &[0, 1, 2, 0, 2, 3])
            .unwrap();
        for triangle in &triangles {
            for start in (0..height).step_by(RASTER_ROWS_PER_STEP as usize) {
                chunked.draw_triangle_rows(
                    &state,
                    Some(texture),
                    *triangle,
                    start,
                    (start + RASTER_ROWS_PER_STEP).min(height) - 1,
                );
            }
        }
        assert_eq!(
            chunked.rgba, scalar.rgba,
            "row chunks diverged before band ownership"
        );
        let banded_shaded = banded.draw_staged_banded_scalar(state, Some(texture), triangles);
        assert_eq!(banded_shaded as u64, scalar_stats.shaded_pixels);
        assert!(
            banded
                .rgba
                .iter()
                .zip(&scalar.rgba)
                .position(|(left, right)| left != right)
                .is_none(),
            "first differing rgba byte: {:?}",
            banded
                .rgba
                .iter()
                .zip(&scalar.rgba)
                .position(|(left, right)| left != right),
        );
        assert_eq!(
            banded.depth.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            scalar.depth.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        );
    }

    /// A repeatable host-side release fixture for the hot menu case: an opaque
    /// full-frame, linearly filtered UI texture.  It is ignored in normal
    /// tests; invoke with `cargo test -p xpapp raster_throughput_fixture --release
    /// -- --ignored --nocapture` to report megapixels per second.
    #[test]
    #[ignore = "release throughput fixture"]
    fn raster_throughput_fixture() {
        use std::time::Instant;

        const WIDTH: u32 = 1280;
        const HEIGHT: u32 = 720;
        const DRAWS: u32 = 24;
        let mut pixels = vec![0; 256 * 256 * 4];
        for (i, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[
                (i as u8).wrapping_mul(17),
                (i as u8).wrapping_mul(31),
                (i as u8).wrapping_mul(47),
                255,
            ]);
        }
        let levels = [TextureLevel {
            width: 256,
            height: 256,
            rgba: &pixels,
        }];
        let texture = TextureView {
            levels: &levels,
            format: TextureFormat::Rgba,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
        };
        let state = RasterState {
            viewport: [0, 0, WIDTH as i32, HEIGHT as i32],
            tex_env: TexEnvMode::Replace,
            ..Default::default()
        };
        let quad = [
            v(-1., -1., 0., 1., 0., 0., [1.; 4]),
            v(1., -1., 0., 1., 1., 0., [1.; 4]),
            v(1., 1., 0., 1., 1., 1., [1.; 4]),
            v(-1., 1., 0., 1., 0., 1., [1.; 4]),
        ];
        let mut frame = Frame::new(WIDTH, HEIGHT).unwrap();
        let started = Instant::now();
        let mut shaded = 0u64;
        for _ in 0..DRAWS {
            shaded += frame
                .draw_indexed(&state, Some(texture), &quad, &[0, 1, 2, 0, 2, 3])
                .unwrap()
                .shaded_pixels;
        }
        let seconds = started.elapsed().as_secs_f64();
        eprintln!(
            "raster fixture: {:.1} Mpix/s ({} pixels in {:.3}s)",
            shaded as f64 / seconds / 1_000_000.0,
            shaded,
            seconds,
        );
        assert_eq!(
            shaded,
            u64::from(WIDTH) * u64::from(HEIGHT) * u64::from(DRAWS)
        );
    }

    /// Measures the scalar recurrence replay cost of the owned-band plan on a
    /// host.  TRUEOS runs these same bands concurrently; this fixture isolates
    /// the setup/copy cost before worker scheduling is involved.
    #[test]
    #[ignore = "release timing fixture"]
    fn row_band_replay_overhead_fixture() {
        use std::time::Instant;

        const WIDTH: u32 = 1280;
        const HEIGHT: u32 = 720;
        const DRAWS: u32 = 6;
        let pixels: Vec<u8> = (0..256 * 256)
            .flat_map(|i| [i as u8, (i >> 3) as u8, (i >> 7) as u8, 255])
            .collect();
        let levels = [TextureLevel {
            width: 256,
            height: 256,
            rgba: &pixels,
        }];
        let texture = TextureView {
            levels: &levels,
            format: TextureFormat::Rgba,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
        };
        let state = RasterState {
            viewport: [0, 0, WIDTH as i32, HEIGHT as i32],
            tex_env: TexEnvMode::Replace,
            ..Default::default()
        };
        let quad = [
            v(-1., -1., 0., 1., 0., 0., [1.; 4]),
            v(1., -1., 0., 1., 1., 0., [1.; 4]),
            v(1., 1., 0., 1., 1., 1., [1.; 4]),
            v(-1., 1., 0., 1., 0., 1., [1.; 4]),
        ];
        let triangles = vec![[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]];
        let mut scalar = Frame::new(WIDTH, HEIGHT).unwrap();
        let started = Instant::now();
        for _ in 0..DRAWS {
            scalar
                .draw_indexed(&state, Some(texture), &quad, &[0, 1, 2, 0, 2, 3])
                .unwrap();
        }
        let scalar_elapsed = started.elapsed();
        let mut bands = Frame::new(WIDTH, HEIGHT).unwrap();
        let started = Instant::now();
        for _ in 0..DRAWS {
            bands.draw_staged_banded_scalar(state, Some(texture), triangles.clone());
        }
        let band_elapsed = started.elapsed();
        assert_eq!(bands.rgba, scalar.rgba);
        assert_eq!(
            bands.depth.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            scalar.depth.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        );
        eprintln!(
            "row-band replay fixture: scalar_ms={:.3} banded_scalar_ms={:.3} ratio={:.2}",
            scalar_elapsed.as_secs_f64() * 1000.0,
            band_elapsed.as_secs_f64() * 1000.0,
            band_elapsed.as_secs_f64() / scalar_elapsed.as_secs_f64(),
        );
    }
}
