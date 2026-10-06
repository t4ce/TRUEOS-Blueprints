use std::collections::BTreeMap;

/// Preserve the original all-pairs removal rule, including equal-height ties
/// and higher neighbours that are themselves removed. Only nearby buckets
/// can contain a peak within the strict distance threshold.
pub(super) fn removals(
    peaks: &[([i32; 2], u32)],
    distance_squared: i32,
    mut checkpoint: impl FnMut(),
) -> Vec<bool> {
    if distance_squared <= 0 {
        return vec![false; peaks.len()];
    }
    let width = i64::from(distance_squared).isqrt() + 1;
    let bucket = |position: [i32; 2]| {
        (i64::from(position[0]).div_euclid(width), i64::from(position[1]).div_euclid(width))
    };
    let mut buckets = BTreeMap::<(i64, i64), Vec<usize>>::new();
    for (i, &(position, _)) in peaks.iter().enumerate() {
        buckets.entry(bucket(position)).or_default().push(i);
        checkpoint();
    }
    peaks.iter().enumerate().map(|(i, &(position, height))| {
        checkpoint();
        let (x, y) = bucket(position);
        for bx in x - 1..=x + 1 {
            for by in y - 1..=y + 1 {
                if let Some(candidates) = buckets.get(&(bx, by)) {
                    for &k in candidates {
                        checkpoint();
                        let (other, other_height) = peaks[k];
                        // Neighbouring buckets bound these differences; i64
                        // also avoids the old i32 distance-square overflow.
                        let dx = i64::from(position[0]) - i64::from(other[0]);
                        let dy = i64::from(position[1]) - i64::from(other[1]);
                        if i != k && height <= other_height
                            && dx * dx + dy * dy < i64::from(distance_squared)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }).collect()
}
