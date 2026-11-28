use anyhow::{Context, Result, bail, ensure};
use glam::{Quat, Vec2, Vec3, Vec4};
use std::io::Cursor;

use crate::{
    Controls,
    hair_hull::{HairHull, LockProfile, lock_texture, ribbon_texture},
    hair_look::{DepthBlend, HairLook},
    hair_roots::extend_roots,
    hair_rope::{HairMotion, RopeGuides},
    obstacle::{BODY_OBSTACLES, HEAD, HEAD_CENTER},
    raster::{Alpha, Camera, Finish, Material, Surface, Texture, Vertex},
    solver::{HAIR_REACH_RADIUS, Solver},
    water::WaterPass,
};

const AVATAR: &[u8] = include_bytes!("../assets/avatar.glb");
const BACK_HAIR_ANCHOR: &[u8] = include_bytes!("../assets/back-hair-anchor-surface.glb");
const WORLD_FLOW_DOWN: Vec3 = Vec3::NEG_Y;
const ROOT_INSET: f32 = 0.006;
// Opaque sRGB samples from the authored Back Hair surface, in root-to-tip order.
const BACK_HAIR_PALETTE: [(f32, [u8; 3]); 5] = [
    (0.0, [42, 32, 61]),
    (0.28, [46, 36, 64]),
    (0.48, [98, 80, 117]),
    (0.73, [109, 93, 130]),
    (1.0, [52, 40, 68]),
];
const LOCK_MATERIAL: Material = Material {
    texture: None,
    factor: Vec4::ONE,
    alpha: Alpha::Opaque,
    double_sided: false,
    finish: None,
};
const RIBBON_MATERIAL: Material = Material {
    // EmoModel's flat hair swatch for the experimental ribbon looks.
    factor: Vec4::new(135.0 / 255.0, 83.0 / 255.0, 189.0 / 255.0, 1.0),
    finish: Some(Finish {
        roughness: 0.38,
        metallic: 0.0,
    }),
    ..LOCK_MATERIAL
};

struct Part {
    vertices: Vec<Vertex>,
    triangles: Vec<[u32; 3]>,
    material: Material,
    back_hair: bool,
}

#[derive(Default)]
pub(crate) struct DrawCounts {
    pub(crate) triangles: usize,
    pub(crate) segments: usize,
}

struct ProceduralHair {
    mesh: HairHull,
    blend: DepthBlend,
    lock_texture: Texture,
    ribbon_texture: Texture,
}

impl ProceduralHair {
    fn new(roots: &[Vec3]) -> Self {
        Self {
            mesh: HairHull::default(),
            blend: DepthBlend::new(roots),
            lock_texture: lock_texture(&BACK_HAIR_PALETTE),
            ribbon_texture: ribbon_texture(),
        }
    }

    fn rebuild(&mut self, ropes: &RopeGuides, look: HairLook) {
        let mesh = &mut self.mesh;
        mesh.clear();
        let (half_width, half_depth, tail_hold) = match look {
            HairLook::Locks => (0.0080, 0.0040, 0.0),
            HairLook::Ribbon => (0.0096, 0.0028, 0.43),
            HairLook::SoftMerge => (0.0108, 0.00245, 0.58),
            HairLook::StrongMerge => (0.0124, 0.0021, 0.67),
        };
        let append = |index: usize, points: &mut [Vec3]| {
            let scalp_normal = (points[0] - HEAD_CENTER).normalize_or(Vec3::Z);
            points[0] -= ROOT_INSET * scalp_normal;
            let width_scale = 0.9 + 0.2 * (index % 7) as f32 / 6.0;
            let color_shift = if look == HairLook::Locks {
                let stagger = (index * 17 % 37) as f32 / 36.0 - 0.5;
                0.07 * stagger
            } else {
                0.0
            };
            mesh.append_lock(
                points,
                scalp_normal,
                LockProfile {
                    half_width: half_width * width_scale,
                    half_depth,
                    tail_hold,
                    color_shift,
                },
            );
        };
        if matches!(look, HairLook::SoftMerge | HairLook::StrongMerge) {
            self.blend.clear();
            ropes.for_each_curve(|_, points| self.blend.push(points));
            self.blend.for_each_curve(look, append);
        } else {
            ropes.for_each_curve(append);
        }
    }
}

pub(crate) struct Scene {
    width: u32,
    height: u32,
    parts: Vec<Part>,
    body_bounds: (Vec3, Vec3),
    textures: Vec<Texture>,
    solver: Option<Solver>,
    ropes: Option<RopeGuides>,
    hull: Option<ProceduralHair>,
    water: Option<WaterPass>,
    camera_scale: Option<f32>,
    camera_center: Option<Vec3>,
    base_key: Option<([f32; 4], bool, bool)>,
    base_pixels: Vec<u8>,
    base_depth: Vec<f32>,
    depth: Vec<f32>,
}

impl Scene {
    pub(crate) fn new(width: u32, height: u32, controls: Controls) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(AVATAR).context("parse embedded reference avatar")?;
        let blob = asset
            .blob
            .as_deref()
            .context("avatar GLB has no embedded buffer")?;
        let textures = asset
            .images()
            .map(|image| match image.source() {
                gltf::image::Source::View {
                    view,
                    mime_type: "image/png",
                } => decode_texture(&blob[view.offset()..view.offset() + view.length()]),
                _ => bail!("expected an embedded PNG atlas"),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut parts = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .map(|primitive| load_part(primitive, blob))
            .collect::<Result<Vec<_>>>()?;
        // Masked/opaque surfaces establish the depth used by eye overlays.
        parts.sort_by_key(|part| matches!(part.material.alpha, Alpha::Blend));
        let rods = controls.hair_motion == Some(HairMotion::Rod) && !controls.water;
        let (mut roots, base_count) = back_hair_roots(rods)?;
        let pose = world_to_avatar(controls);
        let ropes = controls
            .hair_motion
            .filter(|_| !controls.water)
            .map(|motion| {
                let tip_y = parts
                    .iter()
                    .filter(|part| part.back_hair)
                    .flat_map(|part| &part.vertices)
                    .map(|vertex| vertex.position.y)
                    .reduce(f32::min)
                    .unwrap_or(0.75);
                if rods {
                    RopeGuides::with_extra_roots(&roots, base_count, tip_y, pose, motion)
                } else {
                    RopeGuides::new(&roots, tip_y, pose, motion)
                }
            });
        let hull = rods.then(|| ProceduralHair::new(&roots));
        let solver = (ropes.is_none() || controls.layers.field).then(|| {
            roots.truncate(base_count);
            Solver::new(WORLD_FLOW_DOWN, pose, roots)
        });
        let body_bounds = parts.iter().flat_map(|part| &part.vertices).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(minimum, maximum), vertex| {
                (minimum.min(vertex.position), maximum.max(vertex.position))
            },
        );
        let viewport = Vec2::new(width as f32, height as f32);
        let unrolled_camera = |yaw| {
            Camera::new(
                viewport,
                yaw,
                Quat::IDENTITY,
                body_bounds,
                HEAD_CENTER,
                HAIR_REACH_RADIUS,
                None,
            )
        };
        // Keep the established Ubuntu magnification and spin around the center
        // of the unrolled view.
        let camera_scale =
            cfg!(target_os = "linux").then(|| unrolled_camera(165.0_f32.to_radians()).scale);
        let camera_center = cfg!(target_os = "linux").then(|| unrolled_camera(controls.yaw).center);
        let count = width as usize * height as usize;
        Ok(Self {
            width,
            height,
            parts,
            body_bounds,
            textures,
            solver,
            ropes,
            hull,
            water: controls.water.then(WaterPass::default),
            camera_scale,
            camera_center,
            base_key: None,
            base_pixels: vec![0; count * 4],
            base_depth: vec![0.0; count],
            depth: vec![0.0; count],
        })
    }

    pub(crate) fn advance(&mut self, controls: Controls) {
        let pose = world_to_avatar(controls);
        if let Some(solver) = &mut self.solver {
            solver.step(pose, controls.tip_spread, controls.flow);
        }
        if let Some(ropes) = &mut self.ropes {
            ropes.advance(pose, controls.rod_tuning);
        }
        if let Some(water) = &mut self.water {
            water.advance(controls.flow.speed);
        }
    }

    pub(crate) fn guide_count(&self) -> usize {
        self.ropes.as_ref().map_or_else(
            || self.solver.as_ref().map_or(0, Solver::guide_count),
            RopeGuides::len,
        )
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let count = width as usize * height as usize;
        self.base_pixels.resize(count * 4, 0);
        self.base_depth.resize(count, 0.0);
        self.depth.resize(count, 0.0);
        self.camera_scale = None;
        self.base_key = None;
    }

    pub(crate) fn draw(&mut self, controls: Controls, pixels: &mut [u8]) -> DrawCounts {
        let layers = controls.layers;
        let reference_hair = layers.hair && self.hull.is_none();
        let key = (
            [
                controls.yaw,
                controls.roll,
                controls.tilt,
                controls.view_orbit,
            ],
            layers.avatar,
            reference_hair,
        );
        let mut camera = Camera::new(
            Vec2::new(self.width as f32, self.height as f32),
            controls.yaw,
            camera_rotation(controls),
            self.body_bounds,
            HEAD_CENTER,
            HAIR_REACH_RADIUS,
            self.camera_scale,
        );
        self.camera_scale.get_or_insert(camera.scale);
        if let Some(center) = self.camera_center {
            camera.center = center;
        }
        if self.base_key != Some(key) {
            let mut base = Surface::new(
                self.width,
                self.height,
                &mut self.base_pixels,
                &mut self.base_depth,
            );
            base.clear();
            for part in &self.parts {
                let visible = if part.back_hair {
                    reference_hair
                } else {
                    layers.avatar
                };
                if visible {
                    let texture = part.material.texture.map(|index| &self.textures[index]);
                    base.mesh(
                        camera,
                        &part.vertices,
                        &part.triangles,
                        &part.material,
                        texture,
                    );
                }
            }
            self.base_key = Some(key);
        }
        pixels.copy_from_slice(&self.base_pixels);
        self.depth.copy_from_slice(&self.base_depth);
        let mut target = Surface::new(self.width, self.height, pixels, &mut self.depth);
        if layers.reach {
            target.reach_sphere(camera, HEAD_CENTER, HAIR_REACH_RADIUS);
        }
        let mut drawn = DrawCounts::default();
        if let (Some(water), Some(solver)) = (&mut self.water, &self.solver) {
            water.rebuild(solver, controls.water_volume);
            target.water_mesh(camera, &water.vertices, &water.triangles);
            drawn.triangles = water.triangles.len();
        } else {
            if layers.hair
                && let (Some(hull), Some(ropes)) = (&mut self.hull, &self.ropes)
            {
                hull.rebuild(ropes, controls.hair_look);
                let (material, texture) = if controls.hair_look == HairLook::Locks {
                    (&LOCK_MATERIAL, Some(&hull.lock_texture))
                } else {
                    (&RIBBON_MATERIAL, Some(&hull.ribbon_texture))
                };
                target.mesh(
                    camera,
                    &hull.mesh.vertices,
                    &hull.mesh.triangles,
                    material,
                    texture,
                );
                drawn.triangles = hull.mesh.triangles.len();
            }
            if layers.field
                && let Some(solver) = &self.solver
            {
                drawn.segments += solver.for_each_field_line(|start, end, speed, change| {
                    let value = if controls.speed_color { speed } else { change };
                    target.flow_line(camera, start, end, value, 0.24);
                });
            }
            if layers.guides {
                if let Some(ropes) = &self.ropes {
                    drawn.segments += ropes.for_each_line(|start, end, shade| {
                        target.hair_line(camera, start, end, shade);
                    });
                } else if let Some(solver) = &self.solver {
                    let opacity = if cfg!(target_os = "trueos") {
                        0.24
                    } else {
                        0.50
                    };
                    drawn.segments += solver.for_each_guide_line(|_, start, end, speed, change| {
                        let value = if controls.speed_color { speed } else { change };
                        target.flow_line(camera, start, end, value, opacity);
                    });
                }
            }
        }
        if layers.body_obstacles {
            let color = Vec3::new(0.45, 0.77, 0.94);
            for shape in BODY_OBSTACLES {
                target.ellipsoid(camera, shape, color);
            }
        }
        if layers.sphere {
            target.ellipsoid(camera, HEAD, Vec3::ONE);
        }
        drawn
    }
}

fn camera_rotation(controls: Controls) -> Quat {
    Quat::from_rotation_y(controls.view_orbit) * world_to_avatar(controls)
}

#[cfg(target_os = "linux")]
pub(crate) fn headband_fixation_percent(controls: Controls) -> f32 {
    let spawn_direction = world_to_avatar(controls).conjugate() * Vec3::NEG_Y;
    crate::hair_rope::fixation_percent_for(spawn_direction)
}

fn world_to_avatar(controls: Controls) -> Quat {
    let toward = Camera::view_direction(controls.yaw);
    let right = Vec3::Y.cross(toward).normalize();
    let spin = if cfg!(target_os = "linux") {
        Quat::from_axis_angle(toward, controls.roll)
    } else {
        Quat::from_rotation_z(-controls.roll)
    };
    // Inverse of the avatar tilting first, then spinning in the screen plane.
    Quat::from_axis_angle(right, -controls.tilt) * spin
}

fn back_hair_roots(include_extra: bool) -> Result<(Vec<Vec3>, usize)> {
    let asset =
        gltf::Gltf::from_slice(BACK_HAIR_ANCHOR).context("parse back-hair anchor surface")?;
    let blob = asset
        .blob
        .as_deref()
        .context("back-hair anchor has no embedded buffer")?;
    let primitive = asset
        .meshes()
        .find(|mesh| mesh.name() == Some("Back Hair AnkorSurf"))
        .and_then(|mesh| mesh.primitives().next())
        .context("missing Back Hair AnkorSurf mesh")?;
    let reader = primitive.reader(|_| Some(blob));
    let mut positions: Vec<_> = reader
        .read_positions()
        .context("back-hair anchor is missing positions")?
        .map(Vec3::from_array)
        .collect();
    ensure!(
        !positions.is_empty() && positions.iter().all(|position| position.is_finite()),
        "invalid back-hair anchor vertices"
    );
    let base_count = positions.len();
    if include_extra {
        ensure!(
            primitive.mode() == gltf::mesh::Mode::Triangles,
            "expected triangles in the back-hair anchor surface"
        );
        let indices: Vec<_> = reader
            .read_indices()
            .context("back-hair anchor is missing triangle indices")?
            .into_u32()
            .collect();
        let triangles =
            checked_triangles(&indices, base_count).context("back-hair anchor surface")?;
        extend_roots(&mut positions, triangles, base_count / 2);
        ensure!(
            positions.len() == base_count + base_count / 2,
            "back-hair anchor surface could not fit the extra guides"
        );
    }
    Ok((positions, base_count))
}

fn load_part(primitive: gltf::Primitive<'_>, blob: &[u8]) -> Result<Part> {
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "expected triangles in the static avatar"
    );
    let reader = primitive.reader(|_| Some(blob));
    let positions = reader
        .read_positions()
        .context("avatar is missing positions")?;
    let normals = reader.read_normals().context("avatar is missing normals")?;
    let uv = reader
        .read_tex_coords(0)
        .context("avatar is missing UVs")?
        .into_f32();
    ensure!(
        positions.len() == normals.len() && positions.len() == uv.len(),
        "inconsistent mesh vertex streams"
    );
    let vertices: Vec<_> = positions
        .zip(normals)
        .zip(uv)
        .map(|((position, normal), uv)| Vertex {
            position: Vec3::from_array(position),
            normal: Vec3::from_array(normal).normalize_or_zero(),
            uv: Vec2::from_array(uv),
        })
        .collect();
    let indices: Vec<_> = reader
        .read_indices()
        .context("avatar is missing triangle indices")?
        .into_u32()
        .collect();
    let triangles = checked_triangles(&indices, vertices.len()).context("avatar mesh")?;
    let source = primitive.material();
    let pbr = source.pbr_metallic_roughness();
    let material = Material {
        texture: pbr
            .base_color_texture()
            .map(|info| info.texture().source().index()),
        factor: Vec4::from_array(pbr.base_color_factor()),
        alpha: match source.alpha_mode() {
            gltf::material::AlphaMode::Opaque => Alpha::Opaque,
            gltf::material::AlphaMode::Mask => Alpha::Mask(source.alpha_cutoff().unwrap_or(0.5)),
            gltf::material::AlphaMode::Blend => Alpha::Blend,
        },
        double_sided: source.double_sided(),
        finish: (!source.unlit()).then(|| Finish {
            roughness: pbr.roughness_factor(),
            metallic: pbr.metallic_factor(),
        }),
    };
    Ok(Part {
        vertices,
        triangles: triangles.to_vec(),
        material,
        back_hair: source.name() == Some("Back Hair"),
    })
}

fn checked_triangles(indices: &[u32], vertices: usize) -> Result<&[[u32; 3]]> {
    let (triangles, extra) = indices.as_chunks::<3>();
    ensure!(
        extra.is_empty() && indices.iter().all(|&index| (index as usize) < vertices),
        "invalid triangle indices"
    );
    Ok(triangles)
}

fn decode_texture(bytes: &[u8]) -> Result<Texture> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let mut bytes = vec![
        0;
        reader
            .output_buffer_size()
            .context("avatar PNG too large")?
    ];
    let image = reader.next_frame(&mut bytes)?;
    let bytes = &bytes[..image.buffer_size()];
    let pixels = match image.color_type {
        png::ColorType::Rgba => bytes.as_chunks::<4>().0.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        _ => bail!("expected an RGB or RGBA atlas"),
    };
    Ok(Texture {
        width: image.width,
        height: image.height,
        pixels,
    })
}

#[cfg(test)]
fn reference_back_hair() -> Part {
    let asset = gltf::Gltf::from_slice(AVATAR).unwrap();
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some("Back Hair"))
        .unwrap();
    load_part(primitive, asset.blob.as_deref().unwrap()).unwrap()
}

#[cfg(test)]
pub(crate) fn reference_roots() -> Vec<Vec3> {
    back_hair_roots(false).unwrap().0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_budget_reaches_the_hull_from_the_authored_surface() {
        let (roots, base) = back_hair_roots(true).unwrap();
        assert_eq!(base, 150);
        assert_eq!(&roots[..base], reference_roots());
        assert_eq!(roots.len(), 225);

        let mut controls = Controls {
            hair_motion: Some(HairMotion::Rod),
            ..Controls::default()
        };
        controls.layers.body_obstacles = false;
        controls.layers.guides = false;
        controls.layers.field = false;
        let mut scene = Scene::new(240, 180, controls).unwrap();
        let mut pixels = vec![0; 240 * 180 * 4];
        scene.advance(controls);
        let baseline = scene.draw(controls, &mut pixels).triangles;
        for step in 1..=5 {
            controls.rod_tuning.budget_step = step;
            scene.advance(controls);
            assert_eq!(scene.guide_count(), base + 15 * usize::from(step));
        }
        let grown = scene.draw(controls, &mut pixels);
        assert!(grown.triangles > baseline);
        assert_eq!(grown.segments, 0);
        controls.rod_tuning.budget_step = 0;
        scene.advance(controls);
        assert_eq!(scene.guide_count(), base);
        assert_eq!(scene.draw(controls, &mut pixels).triangles, baseline);
    }

    #[test]
    fn accessory_primitives_use_their_authored_finishes() {
        let asset = gltf::Gltf::from_slice(AVATAR).unwrap();
        let blob = asset.blob.as_deref().unwrap();
        let accessory = asset
            .meshes()
            .find(|mesh| mesh.name() == Some("Hair Accessories"))
            .unwrap();
        let mut parts = accessory
            .primitives()
            .map(|part| load_part(part, blob).unwrap());
        let stars = parts.next().unwrap();
        let pins = parts.next().unwrap();
        assert!(parts.next().is_none());
        assert_eq!(stars.material.texture, pins.material.texture);
        for (part, count, roughness, metallic) in [(stars, 19, 0.24, 0.90), (pins, 34, 0.65, 0.0)] {
            let finish = part.material.finish.unwrap();
            assert_eq!(part.triangles.len(), count);
            assert_eq!((finish.roughness, finish.metallic), (roughness, metallic));
            assert!(matches!(part.material.alpha, Alpha::Mask(0.5)));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mouse_roll_turns_clockwise_in_the_screen_plane() {
        let controls = Controls {
            roll: core::f32::consts::FRAC_PI_2,
            ..Controls::default()
        };
        let toward = Camera::view_direction(controls.yaw);
        let right = Vec3::Y.cross(toward).normalize();
        let up = toward.cross(right);
        let spin = world_to_avatar(controls);
        assert!((spin * toward).distance(toward) < 1e-6);
        assert!((spin * right).distance(up) < 1e-6);
        assert!((spin * up).distance(-right) < 1e-6);
    }

    #[test]
    fn tilt_reaches_horizontal_toward_and_away_from_the_camera() {
        for direction in [-1.0, 1.0] {
            let controls = Controls {
                tilt: direction * crate::TILT_LIMIT,
                ..Controls::default()
            };
            let toward = Camera::view_direction(controls.yaw);
            let avatar_up = world_to_avatar(controls).conjugate() * Vec3::Y;
            assert!(avatar_up.y.abs() < 1e-6);
            assert!(direction * avatar_up.dot(toward) > 0.99);
        }
    }

    #[test]
    fn camera_orbit_turns_around_avatar_up_without_changing_pose() {
        let controls = Controls {
            yaw: 0.31,
            roll: 1.2,
            tilt: 0.6,
            ..Controls::default()
        };
        let side = Controls {
            view_orbit: core::f32::consts::FRAC_PI_2,
            ..controls
        };
        let toward = Camera::view_direction(controls.yaw);
        let initial_view = camera_rotation(controls) * toward;
        let side_view = camera_rotation(side) * toward;
        assert_eq!(world_to_avatar(side), world_to_avatar(controls));
        assert!((initial_view.y - side_view.y).abs() < 1e-6);
        assert!(
            Vec2::new(initial_view.x, initial_view.z)
                .dot(Vec2::new(side_view.x, side_view.z))
                .abs()
                < 1e-6
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn viewing_an_inverted_avatar_from_the_side_keeps_the_inverted_cap() {
        let upright = Controls::default();
        let inverted = Controls {
            roll: core::f32::consts::PI,
            ..upright
        };
        assert_eq!(headband_fixation_percent(upright), 50.0);
        assert_eq!(headband_fixation_percent(inverted), 0.0);
        assert_eq!(
            headband_fixation_percent(Controls {
                view_orbit: core::f32::consts::FRAC_PI_2,
                ..inverted
            }),
            0.0
        );
    }

    #[test]
    fn evaluation_sphere_fits_close_to_the_resting_back_hair() {
        let hair = reference_back_hair();
        let radius = hair
            .vertices
            .iter()
            .map(|vertex| vertex.position.distance(HEAD_CENTER))
            .fold(0.0, f32::max);
        assert!((radius..radius + 0.03).contains(&HAIR_REACH_RADIUS));
    }

    #[test]
    fn roots_use_all_authored_vertices() {
        let roots = reference_roots();
        assert_eq!(roots.len(), 150);
        let (min, max) = roots.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), &root| (min.min(root), max.max(root)),
        );
        let span = max - min;
        assert!((0.12..0.17).contains(&span.x));
        assert!((0.08..0.11).contains(&span.y));
        assert!((0.05..0.08).contains(&span.z));
        assert!(min.y < HEAD_CENTER.y && max.y > HEAD_CENTER.y && min.z > HEAD_CENTER.z);
    }
}
