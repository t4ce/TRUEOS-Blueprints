use std::ops::Range;

use glam::{Vec2, Vec3};

use super::{
    COLLAR_ROW, COLLAR_TRIM_START, Fit, HALF_WIDTH_METERS, JOIN_GAP_METERS, LowerFit, LowerStyle,
    OPENING_ROW, OPENING_ROWS_PER_METER, OPENING_TRIM_END, OPENING_WIDTH_METERS,
    PANT_TRIM_FADE_METERS, Surface, TopStyle, bottom, chart, leg_phase, smooth, stitch_along, trim,
};

pub(crate) const ATLAS_WIDTH: usize = 512;
pub(crate) const MAX_ROWS: usize = 768;
const OPENING_TEXEL_METERS: f32 = 0.0011;
const COLLAR_TEXEL_METERS: f32 = 0.0007;

type IndexBytes = [[u8; 4]; 3];

#[derive(Default)]
pub(crate) struct Finish {
    pub(crate) rows: usize,
    patches: Vec<Patch>,
}

struct Patch {
    chart: chart::Chart,
    sources: [usize; 3],
    per_texel: [Vec3; 2],
    original_indices: IndexBytes,
    texels: Vec<Texel>,
}

struct Texel {
    weights: Vec3,
    material: [f32; Surface::ALL.len()],
    tilt: Vec2,
    tint: Vec3,
}

impl Finish {
    pub(crate) fn build<F>(
        style: TopStyle,
        fits: &[Fit],
        body_start: usize,
        body_triangles: Range<usize>,
        triangles: &[[u32; 3]],
        tint_for: impl Fn(usize) -> F,
    ) -> Option<Self>
    where
        F: Fn(Vec3) -> Vec3,
    {
        let lower = style.lower_style();
        let positions = |id: usize| triangles[id].map(|i| fits[i as usize - body_start].position);
        let pack = |scale: f32| {
            chart::pack(
                ATLAS_WIDTH,
                body_triangles.clone().filter_map(|id| {
                    let points = positions(id);
                    texel_size(points, style, lower)
                        .map(|texel_meters| (id, points, texel_meters * scale))
                }),
            )
        };
        let scales: &[f32] = match (style.bottoms.is_some(), style.color.is_some()) {
            (true, true) => &[1.0, 1.25, 1.5, 1.75, 2.0],
            (true, false) => &[1.0, 1.25],
            _ => &[1.0],
        };
        let (rows, charts) = scales
            .iter()
            .find_map(|&scale| pack(scale).filter(|(rows, _)| *rows <= MAX_ROWS))?;
        let patches = charts
            .into_iter()
            .map(|chart| {
                let points = positions(chart.id);
                let per_texel = barycentric_steps(chart.uv);
                let steps = per_texel.map(|weights| interpolate(points, weights));
                let tint_at = tint_for(chart.id);
                let texels = chart
                    .texels()
                    .map(|(_, weights)| {
                        let point = interpolate(points, weights);
                        let (sample, tilt) = sample_at(point, steps, style, lower);
                        let tint = if sample.weights[Surface::Skin as usize] > 0.0 {
                            tint_at(weights)
                        } else {
                            Vec3::ONE
                        };
                        Texel {
                            weights,
                            material: sample.weights,
                            tilt,
                            tint,
                        }
                    })
                    .collect();
                Patch {
                    sources: triangles[chart.id].map(|i| i as usize),
                    chart,
                    per_texel,
                    original_indices: [[0; 4]; 3],
                    texels,
                }
            })
            .collect();
        Some(Self { rows, patches })
    }

    pub(crate) fn replace_indices(&mut self, indices: &mut [u8], base: u32) {
        for (i, patch) in self.patches.iter_mut().enumerate() {
            let indices = triangle_indices(indices, patch.chart.id);
            patch.original_indices = *indices;
            let start = base + 3 * i as u32;
            *indices = [start, start + 1, start + 2].map(u32::to_le_bytes);
        }
    }

    pub(crate) fn restore_indices(&self, indices: &mut [u8]) {
        for patch in &self.patches {
            *triangle_indices(indices, patch.chart.id) = patch.original_indices;
        }
    }

    pub(crate) fn vertices(&self) -> impl Iterator<Item = (usize, Vec2)> + '_ {
        self.patches
            .iter()
            .flat_map(|patch| patch.sources.into_iter().zip(patch.chart.uv))
    }

    pub(crate) fn vertex_count(&self) -> usize {
        self.patches.len() * 3
    }

    pub(crate) fn write(
        &self,
        atlas: &mut [u8],
        width: usize,
        ramps: &[Vec<u8>; Surface::ALL.len()],
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        light_column: impl Fn(usize, Vec3) -> f32,
    ) {
        let last_column = ramps[0].len() / 4 - 1;
        for patch in &self.patches {
            let normals = patch.sources.map(|index| Vec3::from_array(normals[index]));
            let columns = Vec3::from_array(std::array::from_fn(|i| {
                light_column(patch.sources[i], normals[i])
            }));
            let positions = patch
                .sources
                .map(|index| Vec3::from_array(positions[index]));
            let steps = patch
                .per_texel
                .map(|weights| interpolate(positions, weights) / patch.chart.texel_meters);
            // Approximate the small light change from the normal tilt at the
            // posed corners and interpolate it together with the base light.
            let [du, dv] = steps.map(|axis| {
                Vec3::from_array(std::array::from_fn(|corner| {
                    let normal = normals[corner];
                    let tangent = axis - normal * normal.dot(axis);
                    let index = patch.sources[corner];
                    let offset = 0.06 * tangent;
                    let negative = light_column(index, (normal - offset).normalize_or_zero());
                    let positive = light_column(index, (normal + offset).normalize_or_zero());
                    (negative - positive) / 0.12
                }))
            });
            let [left, top] = patch.chart.origin;
            let chart_width = patch.chart.size[0];
            for (row, texels) in patch.texels.chunks_exact(chart_width).enumerate() {
                let start = ((top + row) * width + left) * 4;
                let pixels = &mut atlas[start..start + chart_width * 4];
                for (texel, pixel) in texels.iter().zip(pixels.chunks_exact_mut(4)) {
                    let unbumped_column = columns.dot(texel.weights);
                    let flat = (unbumped_column as usize).min(last_column);
                    let skin = texel.material[Surface::Skin as usize];
                    let fabric = texel.material[Surface::Fabric as usize];
                    let sock = texel.material[Surface::Sock as usize];
                    let shirt = texel.material[Surface::Shirt as usize];
                    if fabric == 1.0
                        || sock == 1.0
                        || shirt == 1.0
                        || (skin == 1.0 && texel.tint == Vec3::ONE)
                    {
                        let surface = if fabric == 1.0 {
                            Surface::Fabric
                        } else if sock == 1.0 {
                            Surface::Sock
                        } else if shirt == 1.0 {
                            Surface::Shirt
                        } else {
                            Surface::Skin
                        };
                        pixel.copy_from_slice(&ramps[surface as usize][flat * 4..][..4]);
                        continue;
                    }
                    let bumped = if texel.tilt != Vec2::ZERO {
                        let delta = (du * texel.tilt.x + dv * texel.tilt.y).dot(texel.weights);
                        (unbumped_column + delta - 0.5).clamp(0.0, last_column as f32)
                    } else {
                        flat as f32
                    };
                    let mut rgb = Vec3::ZERO;
                    for (surface, (&weight, ramp)) in texel.material.iter().zip(ramps).enumerate() {
                        if weight == 0.0 {
                            continue;
                        }
                        if surface == Surface::Skin as usize {
                            rgb += pixel_rgb(ramp, flat) * (texel.tint * weight);
                        } else if surface == Surface::Fabric as usize
                            || surface == Surface::Sock as usize
                            || surface == Surface::Shirt as usize
                        {
                            rgb += pixel_rgb(ramp, flat) * weight;
                        } else {
                            let column = bumped as usize;
                            let a = pixel_rgb(ramp, column);
                            let b = pixel_rgb(ramp, (column + 1).min(last_column));
                            rgb += a.lerp(b, bumped.fract()) * weight;
                        }
                    }
                    pixel[..3].copy_from_slice(&rgb.to_array().map(|value| value.round() as u8));
                    pixel[3] = 255;
                }
            }
        }
    }
}

fn triangle_indices(indices: &mut [u8], triangle: usize) -> &mut IndexBytes {
    let (words, _) = indices.as_chunks_mut::<4>();
    let (triangles, _) = words.as_chunks_mut::<3>();
    &mut triangles[triangle]
}

fn interpolate(points: [Vec3; 3], weights: Vec3) -> Vec3 {
    points[0] * weights.x + points[1] * weights.y + points[2] * weights.z
}

fn pixel_rgb(ramp: &[u8], column: usize) -> Vec3 {
    let offset = column * 4;
    Vec3::new(
        ramp[offset].into(),
        ramp[offset + 1].into(),
        ramp[offset + 2].into(),
    )
}

fn barycentric_steps([a, b, c]: [Vec2; 3]) -> [Vec3; 2] {
    let ab = b - a;
    let ac = c - a;
    let area = ab.perp_dot(ac);
    [(ac.y, -ab.y), (-ac.x, ab.x)].map(|(v, w)| Vec3::new(-v - w, v, w) / area)
}

fn near_trim(min: f32, max: f32) -> bool {
    trim_overlap(min, max, OPENING_ROW, OPENING_TRIM_END)
        || trim_overlap(min, max, COLLAR_TRIM_START, COLLAR_ROW)
}

fn trim_overlap(min: f32, max: f32, start: usize, end: usize) -> bool {
    min < end as f32 + 2.0 && max > start as f32 - 2.0
}

fn texel_size([a, b, c]: [Vec3; 3], style: TopStyle, lower: Option<LowerStyle>) -> Option<f32> {
    let probes = [
        a,
        b,
        c,
        (a + b) * 0.5,
        (b + c) * 0.5,
        (c + a) * 0.5,
        (a + b + c) / 3.0,
    ];
    let bounds = |distances: [f32; 7]| {
        distances
            .into_iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
                (min.min(value), max.max(value))
            })
    };
    let shirt_texel = if style.bottoms.is_none() || style.color.is_some() {
        let (min, max) = bounds(probes.map(|point| Fit::at(point).shirt_row(style)));
        if trim_overlap(min, max, COLLAR_TRIM_START, COLLAR_ROW) {
            return Some(COLLAR_TEXEL_METERS);
        }
        trim_overlap(min, max, OPENING_ROW, OPENING_TRIM_END).then_some(OPENING_TEXEL_METERS)
    } else {
        None
    };
    if let Some(lower) = lower {
        let fits = probes.map(|point| lower.fit_at(point));
        let crosses = |distances, size: f32| {
            let (min, max) = bounds(distances);
            min <= size + 0.003 && max >= -0.003
        };
        let (waist_min, waist_max) = bounds(fits.map(|fit| fit.waist));
        let (leg_min, leg_max) = bounds(fits.map(|fit| fit.leg));
        let pant_width = if lower.socks.is_some() {
            HALF_WIDTH_METERS
        } else {
            0.0
        };
        let near_waistband = lower.joined_hem.is_none() && waist_min <= HALF_WIDTH_METERS + 0.003;
        if waist_max >= -0.003
            && leg_max >= -0.003
            && (near_waistband || leg_min <= pant_width + 0.003)
        {
            return Some(COLLAR_TEXEL_METERS);
        }
        let sock_texel = (leg_min < 0.003
            && crosses(
                fits.map(|fit| {
                    fit.sock.map_or(f32::NEG_INFINITY, |(distance, _)| {
                        if fit.gap() <= JOIN_GAP_METERS {
                            -fit.leg
                        } else {
                            distance
                        }
                    })
                }),
                OPENING_WIDTH_METERS,
            ))
        .then_some(OPENING_TEXEL_METERS);
        return shirt_texel.or(sock_texel);
    }
    shirt_texel
}

fn sample_at(
    point: Vec3,
    steps: [Vec3; 2],
    style: TopStyle,
    lower: Option<LowerStyle>,
) -> (trim::Sample, Vec2) {
    let Some(lower) = lower else {
        return shirt_sample(point, steps, style);
    };
    if style.color.is_none() {
        return lower_sample(point, steps, lower);
    }

    let (mut shirt, mut tilt) = shirt_sample(point, steps, style);
    let below_shirt = std::mem::take(&mut shirt.weights[Surface::Skin as usize]);
    if below_shirt == 1.0 {
        return lower_sample(point, steps, lower);
    }
    shirt.weights[Surface::Shirt as usize] =
        std::mem::take(&mut shirt.weights[Surface::Fabric as usize]);
    shirt.weights[Surface::ShirtBand as usize] =
        std::mem::take(&mut shirt.weights[Surface::Band as usize]);
    if below_shirt > 0.0 {
        let (lower, lower_tilt) = lower_sample(point, steps, lower);
        for (weight, under) in shirt.weights.iter_mut().zip(lower.weights) {
            *weight += under * below_shirt;
        }
        tilt += lower_tilt * below_shirt;
    }
    (shirt, tilt)
}

fn shirt_sample(point: Vec3, steps: [Vec3; 2], style: TopStyle) -> (trim::Sample, Vec2) {
    let row = Fit::at(point).shirt_row(style);
    if near_trim(row, row) {
        trim_at(point, steps, style)
    } else {
        (trim::sample(row, 0.0, 0.0, 0.0), Vec2::ZERO)
    }
}

fn lower_sample(point: Vec3, steps: [Vec3; 2], lower: LowerStyle) -> (trim::Sample, Vec2) {
    let pants = lower.pants;
    let fit_at = |point| lower.fit_at(point);
    let fit = fit_at(point);
    let samples = steps.map(|step| [fit_at(point - 0.5 * step), fit_at(point + 0.5 * step)]);
    let gradient =
        |axis: fn(LowerFit) -> f32| Vec2::from_array(samples.map(|[a, b]| axis(b) - axis(a)));
    let waist_gradient = gradient(|fit| fit.waist);
    let leg_gradient = gradient(|fit| fit.leg);
    let sock_gradient = gradient(|fit| fit.sock.map_or(0.0, |(distance, _)| distance));
    let footprint = |gradient: Vec2| gradient.length().clamp(0.0002, 0.004);
    let cover =
        |distance: f32, gradient: Vec2| (0.5 + distance / footprint(gradient)).clamp(0.0, 1.0);
    let leg = cover(fit.leg, leg_gradient);
    let pants_cover = leg.min(cover(fit.waist, waist_gradient));

    let gap = fit.gap();
    let joined = gap <= JOIN_GAP_METERS;
    let sock_cover = if joined {
        // Assign the entire shared edge once, using the same footprint on both
        // sides. A sub-texel gap would otherwise leave a line of exposed skin.
        1.0 - leg
    } else {
        fit.sock
            .map_or(0.0, |(distance, _)| cover(distance, sock_gradient))
    }
    .min(1.0 - pants_cover);
    let mut weights = [0.0; Surface::ALL.len()];
    weights[Surface::Skin as usize] = (1.0 - pants_cover - sock_cover).max(0.0);
    weights[Surface::Fabric as usize] = pants_cover;
    weights[Surface::Sock as usize] = sock_cover;

    let mut tilt = Vec2::ZERO;
    let mut draw_ribbon = |distance: f32,
                           gradient: Vec2,
                           fabric: Surface,
                           band: Surface,
                           density: f32,
                           strength: f32,
                           phase: &dyn Fn(Vec3) -> (f32, f32)| {
        let rows_per_meter = OPENING_ROWS_PER_METER * density;
        let row = OPENING_ROW as f32 + distance * rows_per_meter;
        if strength == 0.0
            || !trim_overlap(row, row, OPENING_ROW, OPENING_TRIM_END)
            || weights[fabric as usize] == 0.0
        {
            return;
        }
        let along = |point| {
            let (angle, circumference) = phase(point);
            stitch_along(angle, circumference)
        };
        let along_steps = Vec2::from_array(
            steps.map(|step| stitch_step(along(point + 0.5 * step) - along(point - 0.5 * step))),
        );
        let ribbon = trim::sample(
            row,
            along(point),
            footprint(gradient) * rows_per_meter,
            along_steps.length(),
        );
        let covered = 1.0 - ribbon.weights[Surface::Skin as usize];
        let scale = strength * (weights[fabric as usize] / covered.max(f32::EPSILON)).min(1.0);
        for (source, target) in [
            (Surface::Band, band),
            (Surface::Thread, Surface::Thread),
            (Surface::Outline, Surface::Outline),
        ] {
            let contribution = ribbon.weights[source as usize] * scale;
            weights[fabric as usize] -= contribution;
            weights[target as usize] += contribution;
        }
        weights[fabric as usize] = weights[fabric as usize].max(0.0);
        tilt += ribbon.slope * gradient.normalize_or_zero() * scale;
    };

    if lower.joined_hem.is_none() {
        let waist_circumference = 0.72 - 0.15 * pants.waist.clamp(0.0, 1.0);
        draw_ribbon(
            fit.waist,
            waist_gradient,
            Surface::Fabric,
            Surface::Band,
            2.0,
            1.0,
            &|point| ((point.x / 1.3).atan2(point.z - 0.015), waist_circumference),
        );
    }

    if let Some((sock_distance, sock_height)) = fit.sock {
        // A cuff can touch at its inner arc and still leave a gap on the
        // outside. Fade the pant trim there, while following the visible sock edge.
        let pant_strength = smooth((gap - JOIN_GAP_METERS) / PANT_TRIM_FADE_METERS);
        let (sock_distance, sock_gradient) = if joined {
            (-fit.leg, -leg_gradient)
        } else {
            (sock_distance, sock_gradient)
        };
        let leg_length = if point.x < 0.0 {
            pants.left_leg
        } else {
            pants.right_leg
        };
        let pant_height = bottom::cuff_height(0.84, 0.501, leg_length.clamp(0.0, 1.0));
        draw_ribbon(
            fit.leg,
            leg_gradient,
            Surface::Fabric,
            Surface::Band,
            2.0,
            pant_strength,
            &|point| leg_phase(point, pant_height),
        );
        draw_ribbon(
            sock_distance,
            sock_gradient,
            Surface::Sock,
            Surface::SockBand,
            1.0,
            1.0,
            &|point| leg_phase(point, sock_height),
        );
    }
    (
        trim::Sample {
            weights,
            slope: 0.0,
        },
        tilt,
    )
}

fn stitch_step(delta: f32) -> f32 {
    let pitch = trim::STITCH_PERIOD_METERS;
    (delta + 0.5 * pitch).rem_euclid(pitch) - 0.5 * pitch
}

fn trim_at(point: Vec3, steps: [Vec3; 2], style: TopStyle) -> (trim::Sample, Vec2) {
    let evaluate = |point| Fit::at(point).trim_coordinates(style);
    let (row, along) = evaluate(point);
    let [dx, dy] = steps.map(|step| {
        let (start_row, start_along) = evaluate(point - 0.5 * step);
        let (end_row, end_along) = evaluate(point + 0.5 * step);
        let along = stitch_step(end_along - start_along);
        Vec2::new(end_row - start_row, along)
    });
    let gradient = Vec2::new(dx.x, dy.x);
    let sample = trim::sample(
        row,
        along,
        gradient.length().min(2.0),
        Vec2::new(dx.y, dy.y).length(),
    );
    let tilt = sample.slope * gradient.normalize_or_zero();
    (sample, tilt)
}

#[cfg(test)]
mod tests {
    use super::super::{BottomStyle, Pastel, shirt_hem_height};
    use super::*;

    #[test]
    fn combined_outfit_preserves_a_clear_gap_or_joins_black_pants_to_the_shirt() {
        let bottoms = BottomStyle {
            left_leg: 0.6,
            right_leg: 0.6,
            waist: 0.5,
        };
        let separated = TopStyle {
            bottoms: Some(bottoms),
            color: Some(Pastel::Mint),
            sock_color: Some(Pastel::Mint),
            hem: 0.1,
            socks: Some(0.3),
            ..TopStyle::default()
        };
        let close = TopStyle {
            hem: 0.28,
            ..separated
        };
        let covered = TopStyle {
            hem: 0.65,
            ..separated
        };
        let point = Vec3::new(0.07, 0.0, -0.06);
        let waist = LowerFit::at(point, bottoms, separated.socks).waist;
        let steps = [Vec3::X * 0.0007, Vec3::Y * 0.0007];
        let at = |y, style: TopStyle| {
            sample_at(
                Vec3::new(point.x, y, point.z),
                steps,
                style,
                style.lower_style(),
            )
            .0
            .weights
        };

        let waistband = waist - HALF_WIDTH_METERS * 0.55;
        assert!(at(waistband, separated)[Surface::Band as usize] > 0.9);
        let short_hem = shirt_hem_height(point, separated.hem);
        assert_eq!(
            at((waist + short_hem) * 0.5, separated)[Surface::Skin as usize],
            1.0
        );
        assert!(at(short_hem + 0.011, separated)[Surface::ShirtBand as usize] > 0.9);

        let close_hem = shirt_hem_height(point, close.hem);
        assert!(close_hem - waist > 0.01);
        let joined_pants = close.lower_style();
        assert_eq!(joined_pants.unwrap().joined_hem, Some(close.hem));
        for y in [waistband, (waist + close_hem) * 0.5, close_hem - 0.003] {
            assert_eq!(at(y, close)[Surface::Fabric as usize], 1.0);
            let row = Fit::at(point + Vec3::Y * y).coverage_row(close, joined_pants) as usize;
            assert_eq!(close.surface_at_row(row), Surface::Fabric);
        }
        assert!(at(close_hem + 0.011, close)[Surface::ShirtBand as usize] > 0.9);

        let long_hem = shirt_hem_height(point, covered.hem);
        assert_eq!(at(waistband, covered)[Surface::Shirt as usize], 1.0);
        assert!(at(long_hem + 0.011, covered)[Surface::ShirtBand as usize] > 0.9);
        assert_eq!(at(long_hem - 0.003, covered)[Surface::Fabric as usize], 1.0);
        let row = Fit::at(point + Vec3::Y * waistband).coverage_row(covered, covered.lower_style())
            as usize;
        assert_eq!(covered.surface_at_row(row), Surface::Shirt);
        assert_eq!(
            covered.albedo(Surface::Shirt),
            covered.albedo(Surface::Sock)
        );
        assert_ne!(
            covered.albedo(Surface::ShirtBand),
            covered.albedo(Surface::Band)
        );
    }

    #[test]
    fn pant_waistband_follows_the_hips_and_belly_with_or_without_socks() {
        let steps = [Vec3::X * 0.0007, Vec3::Y * 0.0007];
        for waist in [0.0, 0.5, 1.0] {
            let pants = BottomStyle {
                left_leg: waist,
                right_leg: waist,
                waist,
            };
            for socks in [None, Some(0.0), Some(1.0)] {
                let style = TopStyle {
                    bottoms: Some(pants),
                    socks,
                    ..TopStyle::default()
                };
                let lower = style.lower_style();
                for (x, z) in [(0.0, -0.08), (0.1, 0.015), (0.0, 0.08), (-0.1, 0.015)] {
                    let point = Vec3::new(x, 1.0, z);
                    let edge = point + Vec3::Y * LowerFit::at(point, pants, socks).waist;
                    let in_band = edge - Vec3::Y * (HALF_WIDTH_METERS * 0.55);
                    let (ribbon, tilt) = sample_at(in_band, steps, style, lower);
                    assert!(ribbon.weights[Surface::Band as usize] > 0.9);
                    assert_ne!(tilt, Vec2::ZERO);
                    let (outside, _) = sample_at(edge + Vec3::Y * 0.002, steps, style, lower);
                    assert_eq!(outside.weights[Surface::Skin as usize], 1.0);
                    let (inside, _) = sample_at(edge - Vec3::Y * 0.014, steps, style, lower);
                    assert_eq!(inside.weights[Surface::Fabric as usize], 1.0);

                    let triangle = [in_band, in_band + Vec3::X * 0.001, in_band - steps[1]];
                    assert_eq!(
                        texel_size(triangle, style, lower),
                        Some(COLLAR_TEXEL_METERS)
                    );
                }
            }
        }
    }

    #[test]
    fn pant_and_sock_cuffs_only_trim_the_exposed_edges() {
        let pants = BottomStyle {
            left_leg: 1.0,
            ..BottomStyle::default()
        };
        let steps = [Vec3::X * 0.0007, Vec3::Y * 0.0007];
        let at_leg_edge = |x| {
            let point = Vec3::new(x, 0.50, -0.04);
            point - Vec3::Y * LowerFit::at(point, pants, None).leg
        };
        let left = at_leg_edge(-0.09);
        let right = at_leg_edge(0.09);
        let sample = |point, length| {
            let lower = LowerStyle {
                pants,
                socks: Some(length),
                joined_hem: None,
            };
            lower_sample(point, steps, lower).0.weights
        };
        let left_band = left + Vec3::Y * (HALF_WIDTH_METERS * 0.55);
        let right_band = right + Vec3::Y * (HALF_WIDTH_METERS * 0.55);

        // A high sock crosses the longer pant leg and leaves the shorter one free.
        let covered = sample(left_band, 0.8);
        assert_eq!(covered[Surface::Fabric as usize], 1.0);
        assert_eq!(covered[Surface::Band as usize], 0.0);
        assert!(sample(right_band, 0.8)[Surface::Band as usize] > 0.5);

        // Both cuffs keep their sock-colored ribbon, including the relocated
        // cuff. Charts also need to cover triangles entirely below the overlap.
        let ribbon_depth = OPENING_WIDTH_METERS * 0.55;
        let left_sock_band = left - Vec3::Y * ribbon_depth;
        let (right_sock_distance, _) = LowerFit::at(right, pants, Some(0.8)).sock.unwrap();
        let right_sock_band = right + Vec3::Y * (right_sock_distance - ribbon_depth);
        for point in [left_sock_band, right_sock_band] {
            let sock_side = sample(point, 0.8);
            assert!(sock_side[Surface::SockBand as usize] > 0.9);
            assert_eq!(sock_side[Surface::Fabric as usize], 0.0);
            assert_eq!(sock_side[Surface::Band as usize], 0.0);
            assert_eq!(sock_side[Surface::Skin as usize], 0.0);
        }
        let triangle = [
            left_sock_band,
            left_sock_band + Vec3::X * 0.002,
            left_sock_band - Vec3::Y * 0.002,
        ];
        let joined_style = TopStyle {
            bottoms: Some(pants),
            socks: Some(0.8),
            ..TopStyle::default()
        };
        assert!(texel_size(triangle, joined_style, joined_style.lower_style()).is_some());

        // An exact join splits a pixel between pants and the outer sock
        // outline. The shared coverage keeps the skin fully covered.
        let sock_rise = 0.780 - 0.115;
        let (short_sock, _) = LowerFit::at(left, pants, Some(0.0)).sock.unwrap();
        let touching = sample(left, -short_sock / sock_rise);
        assert!(touching[Surface::Fabric as usize] > 0.49);
        assert!(touching[Surface::Outline as usize] > 0.49);
        assert_eq!(touching[Surface::Skin as usize], 0.0);
        assert_eq!(touching[Surface::Band as usize], 0.0);
        assert_eq!(touching[Surface::Thread as usize], 0.0);
    }

    #[test]
    fn avatar_trim_builds_shades_and_restores_the_original_triangles() {
        let glb = gltf::Gltf::from_slice(crate::HUMANOID_GLB).unwrap();
        let body = glb
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(crate::mesh::BODY_MATERIAL))
            .unwrap();
        let reader = body.reader(|_| glb.blob.as_deref());
        let positions: Vec<_> = reader.read_positions().unwrap().collect();
        let normals: Vec<_> = reader.read_normals().unwrap().collect();
        let fits: Vec<_> = positions
            .iter()
            .copied()
            .map(Vec3::from_array)
            .map(Fit::at)
            .collect();
        let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
        let triangles = indices.as_chunks::<3>().0;
        let original: Vec<_> = indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let style = TopStyle {
            color: Some(super::super::Pastel::Lavender),
            left_sleeve: 1.0,
            hem: 1.0,
            ..TopStyle::default()
        };
        let mut finish = Finish::build(style, &fits, 0, 0..triangles.len(), triangles, |_| {
            |_| Vec3::ONE
        })
        .unwrap();
        let mut remapped = original.clone();
        finish.replace_indices(&mut remapped, positions.len() as u32);
        assert_ne!(remapped, original);
        assert_eq!(finish.vertices().count(), finish.vertex_count());

        let color = style.color.unwrap();
        let ramps = Surface::ALL
            .map(|surface| crate::skin_study::color_lookup_rgba(surface.albedo(color.rgb())));
        let mut atlas = vec![0; ATLAS_WIDTH * finish.rows * 4];
        let mut flipped = atlas.clone();
        let column = |direction: Vec3, normal: Vec3| {
            let max = (crate::skin_study::SIZE - 1) as f32;
            (normal.dot(direction).clamp(-1.0, 1.0) + 1.0) * 0.5 * max + 0.5
        };
        finish.write(
            &mut atlas,
            ATLAS_WIDTH,
            &ramps,
            &positions,
            &normals,
            |_, normal| column(Vec3::Y, normal),
        );
        finish.write(
            &mut flipped,
            ATLAS_WIDTH,
            &ramps,
            &positions,
            &normals,
            |_, normal| column(-Vec3::Y, normal),
        );
        assert!(atlas.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert_ne!(atlas, flipped);
        finish.restore_indices(&mut remapped);
        assert_eq!(remapped, original);
    }
}
