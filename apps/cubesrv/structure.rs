//! Server-authored Key8 world 2: 27 red 64-c1 cubes, centered on the origin.
pub fn world() -> cubes_protocol::world::World {
    use cubes_protocol::world::{Cube, World};
    let mut cubes = alloc::vec::Vec::with_capacity(27);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                cubes.push(Cube {
                    min: [x, y, z].map(|v| v * 384 - 192),
                    side: 384,
                    material: 0,
                });
            }
        }
    }
    World {
        cubes,
        spawn: [0, 576, 0],
        normal: [0, 1, 0],
        side_c1: 9216,
    }
}
