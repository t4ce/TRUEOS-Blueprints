//! Server-owned Key5 world roster and atomic geometry/LOD response.
use alloc::vec::Vec;
pub const COUNT: usize = 27;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const NAMES: [&str; COUNT] = [
    "world_01_sky.cubes",
    "world_02_underground.cubes",
    "world_03_black-hole.cubes",
    "world_04_white-hole.cubes",
    "world_05_island.cubes",
    "world_06_city.cubes",
    "world_07_sky_black-hole.cubes",
    "world_08_sky_white-hole.cubes",
    "world_09_sky_island.cubes",
    "world_10_sky_city.cubes",
    "world_11_underground_black-hole.cubes",
    "world_12_underground_white-hole.cubes",
    "world_13_underground_island.cubes",
    "world_14_underground_city.cubes",
    "world_15_black-hole_island.cubes",
    "world_16_black-hole_city.cubes",
    "world_17_white-hole_island.cubes",
    "world_18_white-hole_city.cubes",
    "world_19_sky_black-hole_island.cubes",
    "world_20_sky_black-hole_city.cubes",
    "world_21_sky_white-hole_island.cubes",
    "world_22_sky_white-hole_city.cubes",
    "world_23_underground_black-hole_island.cubes",
    "world_24_underground_black-hole_city.cubes",
    "world_25_underground_white-hole_island.cubes",
    "world_26_underground_white-hole_city.cubes",
    "world_27_void.cubes",
];
#[derive(serde::Serialize, serde::Deserialize)]
pub struct World {
    pub id: u8,
    pub cubes: Vec<u8>,
    pub platforms: Platforms,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Platforms {
    pub filename: alloc::string::String,
    pub decoded: usize,
    pub hulls: Vec<Hull>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hull {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub center: [f32; 3],
    pub side: f32,
    pub rgb: [u8; 3],
    pub ranges: Vec<[usize; 2]>,
}
