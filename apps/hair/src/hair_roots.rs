use glam::Vec3;

pub(crate) fn extend_roots(roots: &mut Vec<Vec3>, triangles: &[[u32; 3]], extra: usize) {
    if extra == 0 {
        return;
    }

    let mut total_area = 0.0;
    let surface: Vec<_> = triangles
        .iter()
        .filter_map(|triangle| {
            let [a, b, c] = triangle.map(|index| roots[index as usize]);
            let area = (b - a).cross(c - a).length() * 0.5;
            if area > 0.0 {
                total_area += area;
                Some(([a, b, c], total_area))
            } else {
                None
            }
        })
        .collect();
    if surface.is_empty() {
        return;
    }

    // Sample by surface area, then add candidates in furthest-first order so
    // every budget prefix fills the remaining gaps between the authored roots.
    let sample_count = (32 * extra).max(512);
    let mut candidates = Vec::with_capacity(sample_count);
    let mut triangle = 0;
    for sample in 0..sample_count {
        let area = total_area * ((sample as f32 + 0.5) / sample_count as f32);
        while surface[triangle].1 < area {
            triangle += 1;
        }
        let [a, b, c] = surface[triangle].0;
        let radius = radical_inverse(sample + 1, 2).sqrt();
        let across = radical_inverse(sample + 1, 3);
        let point = a * (1.0 - radius) + b * (radius * (1.0 - across)) + c * (radius * across);
        let distance = roots
            .iter()
            .map(|root| root.distance_squared(point))
            .fold(f32::INFINITY, f32::min);
        candidates.push((point, distance));
    }

    for _ in 0..extra {
        let Some((index, _)) = candidates
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.1.total_cmp(&b.1))
        else {
            break;
        };
        let (root, distance) = candidates.swap_remove(index);
        if distance <= 1e-12 {
            break;
        }
        roots.push(root);
        for (point, nearest) in &mut candidates {
            *nearest = nearest.min(point.distance_squared(root));
        }
    }
}

fn radical_inverse(mut index: usize, base: usize) -> f32 {
    let step = 1.0 / base as f32;
    let mut factor = step;
    let mut value = 0.0;
    while index > 0 {
        value += (index % base) as f32 * factor;
        index /= base;
        factor *= step;
    }
    value
}
