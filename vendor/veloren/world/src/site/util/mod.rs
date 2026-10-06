pub mod gradient;
pub mod sprites;

use vek::*;

pub fn aabb_iter(aabb: Aabb<i32>) -> impl Iterator<Item = Vec3<i32>> {
    (aabb.min.x..aabb.max.x).flat_map(move |x| {
        (aabb.min.y..aabb.max.y)
            .flat_map(move |y| (aabb.min.z..aabb.max.z).map(move |z| Vec3::new(x, y, z)))
    })
}
