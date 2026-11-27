use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use trueos::{
    vgpu::{Device, RetainedCamera, Ui4Surface},
    vmedia::{self, ImageFormat},
};

use super::{
    compact_mesh,
    eye_detail::{self, EyeDetail},
    floor, floor_material,
    foot_contact::FootProbe,
    hand_contact::HandProbe,
    reel::{GROUND_Y, PlacedPose, Reel},
    reel_draw::{ReelDraw, VERTEX_BYTES},
    report,
    skin_study::{self, Lighting, Material},
};
use crate::{
    animation::Animation,
    autobs::{ARC_LIMIT, BASE_YAW, camera_back, view_back},
    hair::{Hair, PARTICLES},
    hair_body::BodyBack,
    hair_ribbon,
    light::{self, DiffuseLookup, LightPlane},
    mesh::Mesh,
};

const LIGHT_SIZE: usize = skin_study::SIZE;
const LIGHT_ROW: usize = floor_material::SWATCH_ROWS;
const EYE_ROW: usize = LIGHT_ROW + LIGHT_SIZE;
const CONTACT_ROW: usize = EYE_ROW + eye_detail::HEIGHT;
const ATLAS_HEIGHT: usize = 512;
const MAX_ATLAS_WIDTH: usize = 1024;
const ATLAS_WIDTHS: [usize; 2] = [512, MAX_ATLAS_WIDTH];

pub(super) enum Motion {
    Reel(Reel),
    Nod(Animation),
}

impl Motion {
    fn frame_count(&self) -> usize {
        match self {
            Self::Reel(reel) => reel.frame_count(),
            Self::Nod(nod) => (nod.duration() * super::HZ as f32).round() as usize + 1,
        }
    }

    fn phase(&self, frame: usize) -> &'static str {
        match self {
            Self::Reel(reel) => reel.phase(frame),
            Self::Nod(_) => "Nod",
        }
    }

    fn sample(&mut self, frame: usize) -> Result<PlacedPose<'_>> {
        match self {
            Self::Reel(reel) => reel.sample(frame),
            Self::Nod(nod) => {
                nod.apply(frame as f32 / super::HZ as f32);
                let head_from_bind = nod.head_from_bind().context("missing Nod head joint")?;
                let pose = nod.skin();
                Ok(PlacedPose {
                    positions: pose.positions,
                    normals: pose.normals,
                    head_from_bind,
                })
            }
        }
    }
}

fn skin_materials(motion: &Motion, mesh: &Mesh) -> Result<Vec<Material>> {
    let mut materials = Vec::with_capacity(mesh.material_vertices[1].end);
    for (section, range) in mesh.material_vertices[..2].iter().enumerate() {
        let start = materials.len();
        match motion {
            Motion::Reel(reel) => materials.extend(reel.bind_positions(section).map(Material::at)),
            Motion::Nod(nod) => materials.extend(nod.bind_positions(section).map(Material::at)),
        }
        ensure!(
            start == range.start && materials.len() == range.end,
            "skin study rig changed"
        );
    }
    Ok(materials)
}

pub(super) struct Preview<'a> {
    mesh: &'a Mesh,
    motion: Motion,
    draw: ReelDraw,
    source_vertices: Vec<usize>,
    material_slots: Vec<Option<usize>>,
    eye_detail: EyeDetail,
    body_start: usize,
    hair: Hair,
    settled: Hair,
    drape: Vec<[Vec3; PARTICLES]>,
    opening_drape: Vec<[Vec3; PARTICLES]>,
    body: BodyBack,
    previous_pose: Option<usize>,
    feet: FootProbe,
    hands: HandProbe,
    contacts: floor::Atlas,
    skin_materials: Option<Vec<Material>>,
    light_pixels: Vec<u8>,
    light_plane: LightPlane,
    vertices: Vec<u8>,
    indices: Vec<u8>,
    geometry_index_bytes: usize,
    texture: Vec<u8>,
}

impl<'a> Preview<'a> {
    pub(super) async fn new(
        device: Device,
        mesh: &'a Mesh,
        mut motion: Motion,
        glb: &[u8],
        studying_skin: bool,
    ) -> Result<(Self, Vec3, [f32; 2])> {
        let body_start = mesh.material_vertices[1].start;
        let feet = FootProbe::new(glb, body_start)?;
        let hands = HandProbe::new(glb, body_start)?;
        let eye_detail = EyeDetail::load(glb).await?;
        ensure!(
            eye_detail.rgba().len() == eye_detail::WIDTH * eye_detail::HEIGHT * 4,
            "wrong reel eye detail extent"
        );
        let (source_vertices, compact_indices) = compact_mesh(mesh);
        let eye_base = source_vertices.len();
        let hair_base = eye_base + eye_detail.len();
        let strand_count = mesh.strand_guides.len();
        let hair_vertices = strand_count * PARTICLES * 2;
        let skin_materials = studying_skin
            .then(|| skin_materials(&motion, mesh))
            .transpose()?;
        let vertex_count = hair_base + hair_vertices + 4 * floor::CONTACTS;
        let mut indices = Vec::with_capacity(
            (compact_indices.len()
                + eye_detail.indices().len()
                + strand_count * PARTICLES * 6
                + floor::CONTACTS * 6)
                * size_of::<u32>(),
        );
        indices.extend(compact_indices.into_iter().flat_map(u32::to_le_bytes));
        let eye_base = u32::try_from(eye_base)?;
        indices.extend(
            eye_detail
                .indices()
                .iter()
                .flat_map(|&index| (eye_base + index).to_le_bytes()),
        );
        for strand in 0..strand_count {
            let base = (hair_base + strand * PARTICLES * 2) as u32;
            indices.extend(
                hair_ribbon::TRIANGLES
                    .into_iter()
                    .flatten()
                    .flat_map(|i| (base + i).to_le_bytes()),
            );
        }
        let mut material_slots = vec![None; mesh.vertex_count()];
        for (slot, vertices) in mesh.material_vertices[2..].iter().enumerate() {
            material_slots[vertices.clone()].fill(Some(slot));
        }
        let geometry_index_bytes = indices.len();
        let draw = ReelDraw::new(
            device,
            vertex_count * VERTEX_BYTES,
            geometry_index_bytes + floor::CONTACTS * 6 * size_of::<u32>(),
            4 * MAX_ATLAS_WIDTH * ATLAS_HEIGHT,
        )?;
        let light_pixels = if studying_skin {
            report(format_args!(
                "skin study ready sample=vertex lookup=64x1 roughness=100%"
            ));
            skin_study::lookup_rgba()
        } else {
            vmedia::decode(ImageFormat::Bmp, DiffuseLookup::new().update(55))
                .await
                .map_err(super::gpu_error)
                .context("decode reel lighting")?
                .rgba
        };
        let light_rows = if studying_skin { 1 } else { LIGHT_SIZE };
        ensure!(
            light_pixels.len() == LIGHT_SIZE * light_rows * 4,
            "wrong reel lighting extent"
        );

        let (center, half_span) = frame_bounds(&mut motion, &source_vertices)?;
        let initial = motion.sample(0)?;
        let mut body = BodyBack::default();
        body.update(initial.positions, body_indices(mesh, body_start));
        let mut hair = Hair::new(mesh.strand_guides.clone(), initial.head_from_bind);
        for _ in 0..2 * super::HZ {
            hair.step(initial.head_from_bind, Vec3::ZERO, &body);
        }
        let opening_drape = hair.strands().copied().collect();
        let settled = hair.clone();
        let (clips, playback) = match motion {
            Motion::Reel(_) => (
                "Greeting,Nod,Squat,Spin,Model-Pose,Show-Full-Body,Shoot,Peace-Sign",
                "reel",
            ),
            Motion::Nod(_) => ("Nod", "nod"),
        };
        report(format_args!(
            "avatar ready target_hz={} clips={clips} poses={} playback={playback}",
            super::HZ,
            motion.frame_count()
        ));
        Ok((
            Self {
                mesh,
                motion,
                draw,
                source_vertices,
                material_slots,
                eye_detail,
                body_start,
                hair,
                settled,
                drape: Vec::with_capacity(strand_count),
                opening_drape,
                body,
                previous_pose: None,
                feet,
                hands,
                contacts: floor::Atlas::new(1),
                skin_materials,
                light_pixels,
                light_plane: LightPlane::new(),
                vertices: Vec::with_capacity(vertex_count * VERTEX_BYTES),
                indices,
                geometry_index_bytes,
                texture: Vec::new(),
            },
            center,
            half_span,
        ))
    }

    pub(super) fn pose(&self, tick: usize) -> (usize, &'static str) {
        let index = tick % self.motion.frame_count();
        (index, self.motion.phase(index))
    }

    pub(super) fn submit(
        &mut self,
        surface: Ui4Surface,
        camera: RetainedCamera,
        frame: usize,
        elapsed_ns: u64,
    ) -> Result<(), i32> {
        let extent = match self.stage(camera, frame, elapsed_ns) {
            Ok(extent) => extent,
            Err(error) => {
                report(format_args!("prepare reel frame {frame}: {error:#}"));
                return Err(-5);
            }
        };
        self.draw.submit(
            surface,
            &self.vertices,
            &self.indices,
            &self.texture,
            extent,
            floor_material::CLEAR_RGBA,
        )
    }

    fn stage(&mut self, camera: RetainedCamera, frame: usize, elapsed_ns: u64) -> Result<[u32; 2]> {
        let frame_count = self.motion.frame_count();
        if let Some(previous) = self.previous_pose {
            if frame < previous {
                self.hair = self.settled.clone();
            } else {
                // Advance the solver for skipped ticks too, while bounding catch-up work.
                for skipped in (previous + 1).max(frame.saturating_sub(3))..frame {
                    let pose = self.motion.sample(skipped)?;
                    self.body
                        .update(pose.positions, body_indices(self.mesh, self.body_start));
                    self.hair.step(pose.head_from_bind, Vec3::ZERO, &self.body);
                }
            }
        }
        let pose = self.motion.sample(frame)?;
        if frame != 0 && self.previous_pose != Some(frame) {
            self.body
                .update(pose.positions, body_indices(self.mesh, self.body_start));
            self.hair.step(pose.head_from_bind, Vec3::ZERO, &self.body);
        }
        self.previous_pose = Some(frame);

        let [[left_heel, left_toe], [right_heel, right_toe]] =
            self.feet.sample(pose.positions).map(|foot| foot.points());
        self.contacts.clear();
        self.contacts.push(
            GROUND_Y,
            [left_heel, left_toe, right_heel, right_toe],
            self.hands.sample(pose.positions),
        );
        let palette = self
            .contacts
            .bake_rows(&ATLAS_WIDTHS, ATLAS_HEIGHT, CONTACT_ROW)?;
        let [width, height] = palette.extent();
        palette.write_rgba(&mut self.texture);
        let stride = width as usize * 4;
        for (start_row, source_width, pixels) in [
            (LIGHT_ROW, LIGHT_SIZE, self.light_pixels.as_slice()),
            (EYE_ROW, eye_detail::WIDTH, self.eye_detail.rgba()),
        ] {
            for (row, colors) in pixels.chunks_exact(source_width * 4).enumerate() {
                let start = (start_row + row) * stride;
                self.texture[start..start + colors.len()].copy_from_slice(colors);
            }
        }
        let swatches = [palette.swatch(0), palette.swatch(1), palette.swatch(2)];
        let view_projection = Mat4::from_cols_array(&camera.view_projection);
        let back = -Vec3::from_array(
            camera.forward_far[..3]
                .try_into()
                .expect("camera direction"),
        );
        let skin = self
            .skin_materials
            .as_deref()
            .map(|materials| (materials, Lighting::new(light::angle(elapsed_ns), back)));
        self.vertices.clear();
        let output = &mut self.vertices;
        let mut append = |position: Vec3, uv: [f32; 2]| {
            let clip = view_projection.transform_point3(position).to_array();
            for value in clip.into_iter().chain(uv) {
                output.extend_from_slice(&value.to_le_bytes());
            }
        };
        for &index in &self.source_vertices {
            let uv = if let Some(slot) = self.material_slots[index] {
                swatches[slot]
            } else {
                let normal = Vec3::from_array(pose.normals[index]);
                let (u, row) = if let Some((materials, lighting)) = &skin {
                    (lighting.coordinate(normal, materials[index]), 0.5)
                } else {
                    let [u, v] = self.light_plane.coordinates(normal);
                    (u, v * LIGHT_SIZE as f32)
                };
                [
                    u * LIGHT_SIZE as f32 / width as f32,
                    (row + LIGHT_ROW as f32) / height as f32,
                ]
            };
            append(Vec3::from_array(pose.positions[index]), uv);
        }
        for (position, [u, v]) in self.eye_detail.vertices(pose.positions) {
            let uv = [
                u * eye_detail::WIDTH as f32 / width as f32,
                (v * eye_detail::HEIGHT as f32 + EYE_ROW as f32) / height as f32,
            ];
            append(Vec3::from_array(position), uv);
        }
        self.drape.clear();
        let drape_blend_start = frame_count.saturating_sub(super::HZ as usize);
        let t = (frame.saturating_sub(drape_blend_start) as f32 / super::HZ as f32).clamp(0.0, 1.0);
        self.hair.append_loop_pose(
            &self.opening_drape,
            t * t * (3.0 - 2.0 * t),
            &mut self.drape,
        );
        for points in &self.drape {
            for vertex in hair_ribbon::vertices(points, back).into_iter().flatten() {
                append(vertex, swatches[0]);
            }
        }
        let ground_base = (self.source_vertices.len()
            + self.eye_detail.len()
            + self.drape.len() * PARTICLES * 2) as u32;
        self.indices.truncate(self.geometry_index_bytes);
        for (i, (points, uv)) in self.contacts.quads(0, GROUND_Y, &palette).enumerate() {
            for (point, uv) in points.into_iter().zip(uv) {
                append(Vec3::from_array(point), uv);
            }
            self.indices.extend(
                [0, 1, 2, 2, 1, 3]
                    .into_iter()
                    .flat_map(|corner| (ground_base + 4 * i as u32 + corner).to_le_bytes()),
            );
        }
        Ok([width, height])
    }
}

fn body_indices(mesh: &Mesh, body_start: usize) -> &[u32] {
    let skin = &mesh.indices[..mesh.skin_triangle_count * 3];
    &skin[skin.partition_point(|&index| (index as usize) < body_start)..]
}

fn frame_bounds(motion: &mut Motion, source: &[usize]) -> Result<(Vec3, [f32; 2])> {
    let right = Vec3::Y.cross(camera_back(BASE_YAW));
    let back = view_back(BASE_YAW);
    let up = back.cross(right);
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for frame in (0..motion.frame_count()).step_by(5) {
        let pose = motion.sample(frame)?;
        for &index in source {
            let position = Vec3::from_array(pose.positions[index]);
            let point = Vec3::new(right.dot(position), up.dot(position), back.dot(position));
            low = low.min(point);
            high = high.max(point);
        }
    }
    let center = (high + low) * 0.5;
    let half = (high - low) * 0.5;
    let arc_margin = half.z * ARC_LIMIT.sin();
    let span = [half.x + arc_margin + 0.04, half.y + 0.04];
    Ok((right * center.x + up * center.y + back * center.z, span))
}
