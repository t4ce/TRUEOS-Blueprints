//! Six fixed demo locations behind the gallery.

/// Twice the image-center distance, in the same six directions from the origin.
/// Keep size-demo order: +X, +Z, -X, -Z, above, below.
pub fn anchors() -> [[i16;3];6] {
    let d=cubes_protocol::gallery::GALLERY_DISTANCE_TWICE_C1 as i16;
    [[d,0,0],[0,0,d],[-d,0,0],[0,0,-d],[0,d,0],[0,-d,0]]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_bases_are_twice_each_image_center_from_the_origin() {
        use cubes_protocol::gallery::{BASES,GALLERY_DISTANCE_TWICE_C1,C1};
        let a=anchors();
        let image_faces=[1,2,3,0,5,4];
        for (i,face) in image_faces.into_iter().enumerate() {
            let expected=BASES[face][2].map(|v|(-v*GALLERY_DISTANCE_TWICE_C1) as i16);
            assert_eq!(a[i],expected);
            assert_eq!(a[i].iter().filter(|v|**v!=0).count(),1);
            let distance=a[i].iter().map(|v|(*v as i32).pow(2)).sum::<i32>();
            assert_eq!(distance,GALLERY_DISTANCE_TWICE_C1.pow(2));
            assert!((distance as f32).sqrt()*C1==41.);
            for j in i+1..6 {
                let separation=(0..3).map(|k|(a[i][k] as i32-a[j][k] as i32).pow(2)).sum::<i32>();
                assert!(separation>24*24); // terrain width plus two-cube empty gap
            }
        }
        for _ in 0..48 {assert_eq!(anchors(),a);}
    }
}
