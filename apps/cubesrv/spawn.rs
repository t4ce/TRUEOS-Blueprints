//! Four spaced anchors share the existing three-second spawn clock.
pub const INTERVAL_MS: u64 = 3000;

/// Cardinal centers 5..=10 c4 blocks from the landmark, at its top elevation.
/// Adjacent anchors are at least sqrt(2)*40 c1 apart: more than the full
/// 32x32 billboard diagonal, and well over two terrain blocks of clearance.
pub fn anchors(cycle: u64) -> [[i16;3];4] {
    let d=(5+cycle%6) as i16*8;
    [[d,8,0],[0,8,d],[-d,8,0],[0,8,-d]]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_four_planes_and_terrain_cubes_have_clearance() {
        for cycle in 0..48 {
            let a=anchors(cycle);
            for i in 0..4 {
                let radius=a[i][0] as i32*a[i][0] as i32+a[i][2] as i32*a[i][2] as i32;
                assert!((1600..=6400).contains(&radius));
                for j in i+1..4 {
                    let d=(0..3).map(|k| (a[i][k] as i32-a[j][k] as i32).pow(2)).sum::<i32>();
                    assert!(d>2*33*33); // full sprite diagonal plus cube bevel margin
                    assert!(d>24*24); // c4 width plus two-cube empty gap
                }
            }
        }
    }
}
