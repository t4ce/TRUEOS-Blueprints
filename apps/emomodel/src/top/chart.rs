use std::cmp::Reverse;

use glam::{Vec2, Vec3};

pub(crate) struct Chart {
    pub(crate) id: usize,
    pub(crate) uv: [Vec2; 3],
    pub(crate) origin: [usize; 2],
    pub(crate) size: [usize; 2],
    pub(crate) texel_meters: f32,
}

impl Chart {
    fn new(id: usize, positions: [Vec3; 3], texel_meters: f32) -> Option<Self> {
        let [a, b, c] = positions;
        let longest = [b - a, c - b, a - c]
            .into_iter()
            .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?;
        let x = longest.try_normalize()?;
        let y = (b - a).cross(c - a).try_normalize()?.cross(x);
        let uv = positions.map(|p| {
            let delta = p - a;
            Vec2::new(delta.dot(x), delta.dot(y)) / texel_meters
        });
        let [a, b, c] = uv;
        let lo = a.min(b).min(c).floor();
        let hi = a.max(b).max(c).ceil();
        let extent = hi - lo;
        Some(Self {
            id,
            uv: uv.map(|p| p - lo + Vec2::ONE),
            origin: [0, 0],
            size: [extent.x as usize + 2, extent.y as usize + 2],
            texel_meters,
        })
    }

    pub(crate) fn texels(&self) -> impl Iterator<Item = ([usize; 2], Vec3)> + '_ {
        let [width, height] = self.size;
        let [left, top] = self.origin;
        (0..width * height).map(move |texel| {
            let xy = [left + texel % width, top + texel / width];
            let point = Vec2::new(xy[0] as f32 + 0.5, xy[1] as f32 + 0.5);
            (xy, closest_weights(self.uv, point))
        })
    }
}

struct Shelf {
    y: usize,
    used: usize,
}

pub(crate) fn pack(
    width: usize,
    triangles: impl IntoIterator<Item = (usize, [Vec3; 3], f32)>,
) -> Option<(usize, Vec<Chart>)> {
    let mut charts: Vec<_> = triangles
        .into_iter()
        .filter_map(|(id, positions, texel_meters)| Chart::new(id, positions, texel_meters))
        .collect();
    // Each existing shelf is tall enough for every chart that follows it.
    charts.sort_unstable_by_key(|chart| (Reverse(chart.size[1]), Reverse(chart.size[0]), chart.id));
    let mut shelves: Vec<Shelf> = Vec::new();
    let mut total_height = 0;
    for chart in &mut charts {
        let [chart_width, chart_height] = chart.size;
        if chart_width > width {
            return None;
        }
        let shelf = shelves
            .iter()
            .enumerate()
            .filter(|(_, shelf)| shelf.used <= width - chart_width)
            .max_by_key(|(_, shelf)| shelf.used)
            .map(|(index, _)| index)
            .unwrap_or_else(|| {
                let shelf = Shelf {
                    y: total_height,
                    used: 0,
                };
                total_height += chart_height;
                let index = shelves.len();
                shelves.push(shelf);
                index
            });
        let shelf = &mut shelves[shelf];
        chart.origin = [shelf.used, shelf.y];
        let offset = Vec2::new(shelf.used as f32, shelf.y as f32);
        chart.uv = chart.uv.map(|p| p + offset);
        shelf.used += chart_width;
    }
    Some((total_height, charts))
}

fn closest_weights(uv: [Vec2; 3], point: Vec2) -> Vec3 {
    let [a, b, c] = uv;
    let ab = b - a;
    let ac = c - a;
    let area = ab.perp_dot(ac);
    let delta = point - a;
    let v = delta.perp_dot(ac) / area;
    let w = ab.perp_dot(delta) / area;
    let weights = Vec3::new(1.0 - v - w, v, w);
    if weights.min_element() >= 0.0 {
        return weights;
    }

    // Extend edge values into the padded rectangle for texels just outside the chart triangle.
    let mut best = (f32::INFINITY, Vec3::X);
    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
        let edge = uv[b] - uv[a];
        let t = ((point - uv[a]).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
        let distance = point.distance_squared(uv[a] + t * edge);
        if distance < best.0 {
            let mut weights = Vec3::ZERO;
            weights[a] = 1.0 - t;
            weights[b] = t;
            best = (distance, weights);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn charts_keep_metric_scale_and_pad_their_own_edges() {
        let points = [
            Vec3::new(0.002, 0.001, 0.004),
            Vec3::new(0.009, 0.005, 0.003),
            Vec3::new(0.003, 0.006, 0.011),
        ];
        let (height, charts) = pack(
            25,
            [
                (4, points, 0.001),
                (8, points, 0.0015),
                (9, [Vec3::ZERO; 3], 0.001),
            ],
        )
        .unwrap();
        assert_eq!(charts.len(), 2);
        let mut occupied = BTreeSet::new();
        for chart in &charts {
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let meters = chart.uv[a].distance(chart.uv[b]) * chart.texel_meters;
                assert!((meters - points[a].distance(points[b])).abs() < 1e-6);
            }
            let mut gutter = 0;
            for ([x, y], weights) in chart.texels() {
                assert!(x < 25 && y < height);
                assert!(occupied.insert([x, y]));
                assert!(weights.min_element() >= 0.0);
                assert!((weights.element_sum() - 1.0).abs() < 1e-6);
                gutter += usize::from(weights.min_element() == 0.0);
            }
            assert!(gutter > 0);
        }
    }
}
