//! Six spaced anchors share the existing three-second spawn clock.
pub const INTERVAL_MS: u64 = 3000;

/// One of the smallest six pixel sizes per direction. Radial distances grow
/// with the billboard footprint; base terrain cubes retain their c4 size.
pub fn anchors(cycle: u64) -> [[i16;3];6] {
    let d=((5+cycle%6) as i16*8).max(56);
    let sides=cubes_protocol::vfx::DEMO_PIXEL_SIDES_C1.map(i16::from);
    [[d*sides[0],8,0],[0,8,d*sides[1]],[-d*sides[2],8,0],
     [0,8,-d*sides[3]],[0,d*sides[4],0],[0,-d*sides[5],0]]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_six_planes_and_terrain_cubes_have_clearance() {
        for cycle in 0..48 {
            let a=anchors(cycle);
            assert!(a[4][1]>0 && a[5][1]<0);
            assert_eq!([a[4][0],a[4][2],a[5][0],a[5][2]],[0;4]);
            let sides=cubes_protocol::vfx::DEMO_PIXEL_SIDES_C1;
            // Conservative radius around each top anchor covers all viewport rotations.
            let radii=sides.map(|s|(32_f32.powi(2)+16_f32.powi(2)).sqrt()*s as f32+s as f32);
            assert!(a[5][1] as f32+4.+radii[5] < -12.);
            for i in 0..6 {
                assert!(a[i].iter().all(|v|(-1024..=1024).contains(v)));
                for j in i+1..6 {
                    let d=(0..3).map(|k| (a[i][k] as i32-a[j][k] as i32).pow(2)).sum::<i32>();
                    assert!((d as f32)>(radii[i]+radii[j]).powi(2));
                    assert!(d>24*24); // c4 width plus two-cube empty gap
                }
            }
        }
    }
}
