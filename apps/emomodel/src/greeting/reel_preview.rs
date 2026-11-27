use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use trueos::{
    vgpu::{Device, RetainedCamera, Ui4Surface},
    vmedia::{self, ImageFormat},
};

use super::{
    SECOND_NS, compact_mesh,
    dance::Dance,
    eye_detail::{self, EyeDetail},
    eye_sprite::{self, EyeSprite},
    floor, floor_material,
    foot_contact::FootProbe,
    hand_contact::HandProbe,
    heart::Heart,
    lico_graph::{DanceClueSnapshot, Observer},
    pupil::{self, Stage},
    reel::{GROUND_Y, PlacedPose, Reel},
    reel_draw::{ReelDraw, VERTEX_BYTES},
    report,
    skin_study::{self, Lighting, Material},
};
use crate::{
    EyeStyle,
    animation::{Animation, RigPose},
    autobs::{ARC_LIMIT, BASE_YAW, camera_back, view_back},
    hair::{Hair, PARTICLES},
    hair_body::BodyBack,
    hair_ribbon,
    light::{self, DiffuseLookup, LightPlane},
    mesh::Mesh,
};

const LIGHT_SIZE: usize = skin_study::SIZE;
const LIGHT_ROW: usize = floor_material::SWATCH_ROWS;
const IRIS_ROW: usize = LIGHT_ROW;
const IRIS_COLUMN: usize = LIGHT_SIZE + 1;
const EYE_ROW: usize = IRIS_ROW + eye_sprite::SIZE;
const CONTACT_ROW: usize = EYE_ROW + eye_detail::HEIGHT;
const ATLAS_HEIGHT: usize = 512;
const MAX_ATLAS_WIDTH: usize = 1024;
const ATLAS_WIDTHS: [usize; 2] = [512, MAX_ATLAS_WIDTH];
const MODE_BLEND_NS: u64 = 400_000_000;

pub(super) enum Motion {
    Reel(Reel),
    Nod(Animation),
    Heart(Heart),
}

impl Motion {
    fn frame_count(&self) -> usize {
        match self {
            Self::Reel(reel) => reel.frame_count(),
            Self::Nod(nod) => (nod.duration() * super::HZ as f32).round() as usize + 1,
            Self::Heart(heart) => heart.frame_count(),
        }
    }

    fn frame(&self, tick: usize) -> usize {
        if matches!(self, Self::Heart(_)) {
            tick.min(self.frame_count() - 1)
        } else {
            tick % self.frame_count()
        }
    }

    fn phase(&self, frame: usize) -> &'static str {
        match self {
            Self::Reel(reel) => reel.phase(frame),
            Self::Nod(_) => "Nod",
            Self::Heart(heart) => heart.phase(frame),
        }
    }

    fn pupil_stage(&self, frame: usize) -> Stage {
        match self {
            Self::Reel(reel) => reel.pupil_stage(frame),
            Self::Nod(nod) => pupil::nod_stage(frame as f32 / super::HZ as f32, nod.duration()),
            Self::Heart(heart) => heart.pupil_stage(frame),
        }
    }

    fn stroke(&self, frame: usize) -> &[[Vec3; 2]] {
        match self {
            Self::Heart(heart) => heart.stroke(frame),
            _ => &[],
        }
    }

    fn sample(&mut self, frame: usize) -> Result<PlacedPose<'_>> {
        match self {
            Self::Reel(reel) => reel.sample(frame),
            Self::Heart(heart) => heart.sample(frame),
            Self::Nod(nod) => {
                nod.apply(frame as f32 / super::HZ as f32);
                let head_from_bind = nod.head_from_bind().context("missing Nod head joint")?;
                let pose = nod.skin();
                Ok(PlacedPose {
                    positions: pose.positions,
                    normals: pose.normals,
                    head_from_bind,
                    joint_world: pose.joint_world,
                    world_offset: Vec3::ZERO,
                })
            }
        }
    }

    fn apply_bind_normals(&mut self, section: usize, normals: &[[f32; 3]]) -> Result<()> {
        match self {
            Self::Reel(reel) => reel.apply_bind_normals(section, normals),
            Self::Nod(nod) => nod.apply_bind_normals(section, normals),
            Self::Heart(heart) => heart.apply_bind_normals(section, normals),
        }
    }
}

#[derive(Default)]
struct PlaybackClock {
    changed_ns: u64,
    reel_elapsed_ns: u64,
    dancing: bool,
}

impl PlaybackClock {
    fn select(&mut self, dancing: bool, elapsed_ns: u64) {
        if self.dancing == dancing {
            return;
        }
        if dancing {
            self.reel_elapsed_ns = self.motion_time(elapsed_ns);
        }
        self.changed_ns = elapsed_ns;
        self.dancing = dancing;
    }

    fn motion_time(&self, elapsed_ns: u64) -> u64 {
        if self.dancing {
            self.reel_elapsed_ns
        } else {
            self.reel_elapsed_ns + elapsed_ns.saturating_sub(self.changed_ns)
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
            Motion::Heart(heart) => {
                materials.extend(heart.bind_positions(section).map(Material::at))
            }
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
    lico: Option<(Dance, Observer)>,
    dance_clue: Option<Arc<DanceClueSnapshot>>,
    clock: PlaybackClock,
    blend_rig: Animation,
    displayed_pose: RigPose,
    transition: Option<(u64, RigPose)>,
    lifted_positions: Vec<[f32; 3]>,
    draw: ReelDraw,
    source_vertices: Vec<usize>,
    material_slots: Vec<Option<usize>>,
    eye_detail: EyeDetail,
    iris: EyeSprite,
    pupil: pupil::Transition,
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
        eyes: EyeStyle,
    ) -> Result<(Self, Vec3, [f32; 2])> {
        let is_heart = matches!(&motion, Motion::Heart(_));
        let mut dance = (!is_heart)
            .then(|| Dance::new(glb, &super::MATERIALS))
            .transpose()?;
        let mut blend_rig = Animation::procedural(glb, &super::MATERIALS)?;
        if !studying_skin {
            super::apply_normal_maps(glb, |section, normals| {
                motion.apply_bind_normals(section, normals)?;
                if let Some(dance) = &mut dance {
                    dance.apply_bind_normals(section, normals)?;
                }
                blend_rig.apply_bind_normals(section, normals)
            })
            .await?;
        }
        let body_start = mesh.material_vertices[1].start;
        let feet = FootProbe::new(glb, body_start)?;
        let hands = HandProbe::new(glb, body_start)?;
        let eye_detail = EyeDetail::load(glb).await?;
        let iris = EyeSprite::load(eyes).await?;
        ensure!(
            eye_detail.rgba().len() == eye_detail::WIDTH * eye_detail::HEIGHT * 4,
            "wrong reel eye detail extent"
        );
        let (source_vertices, compact_indices) = compact_mesh(mesh);
        let eye_base = source_vertices.len();
        let pupil_base = eye_base + eye_sprite::VERTEX_COUNT as usize;
        let detail_base = pupil_base + eye_sprite::PUPIL_VERTEX_COUNT as usize;
        let hair_base = detail_base + eye_detail.len();
        let strand_count = mesh.strand_guides.len();
        let hair_vertices = strand_count * PARTICLES * 2;
        let skin_materials = studying_skin
            .then(|| skin_materials(&motion, mesh))
            .transpose()?;
        let stroke_pairs = motion.stroke(motion.frame_count() - 1).len();
        let stroke_indices = stroke_pairs.saturating_sub(1) * 6;
        let vertex_count = hair_base + hair_vertices + 4 * floor::CONTACTS + 2 * stroke_pairs;
        let mut indices = Vec::with_capacity(
            (compact_indices.len()
                + eye_sprite::INDEX_COUNT
                + eye_sprite::PUPIL_INDEX_COUNT
                + eye_detail.indices().len()
                + strand_count * PARTICLES * 6
                + floor::CONTACTS * 6
                + stroke_indices)
                * size_of::<u32>(),
        );
        indices.extend(compact_indices.into_iter().flat_map(u32::to_le_bytes));
        indices.extend(
            EyeSprite::indices(u32::try_from(eye_base)?)
                .chain(EyeSprite::pupil_indices(u32::try_from(pupil_base)?))
                .flatten()
                .flat_map(u32::to_le_bytes),
        );
        let detail_base = u32::try_from(detail_base)?;
        indices.extend(
            eye_detail
                .indices()
                .iter()
                .flat_map(|&index| (detail_base + index).to_le_bytes()),
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
            geometry_index_bytes + (floor::CONTACTS * 6 + stroke_indices) * size_of::<u32>(),
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
            Motion::Heart(_) => ("Heart", "gesture"),
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
                lico: dance.map(|dance| (dance, Observer::new())),
                dance_clue: None,
                clock: PlaybackClock::default(),
                blend_rig,
                displayed_pose: RigPose::default(),
                transition: None,
                lifted_positions: Vec::new(),
                draw,
                source_vertices,
                material_slots,
                eye_detail,
                iris,
                pupil: pupil::Transition::default(),
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

    pub(super) fn sync_lico(&mut self, elapsed_ns: u64) {
        let Some((dance, observer)) = &mut self.lico else {
            return;
        };
        let clue = observer.current();
        match (&self.dance_clue, &clue) {
            (None, None) => return,
            (Some(current), Some(next)) if Arc::ptr_eq(current, next) => return,
            _ => {}
        }
        if let Some(snapshot) = &clue {
            dance.set_graph(&snapshot.graph);
            report(format_args!(
                "avatar mode=lico-dance run={} nodes={} patterns={}",
                snapshot.run,
                snapshot.graph.nodes.len(),
                snapshot.graph.patterns.len()
            ));
        } else {
            report(format_args!("avatar mode=reel source=lico-stop"));
        }
        self.clock.select(clue.is_some(), elapsed_ns);
        self.transition = (!self.displayed_pose.joints.is_empty())
            .then(|| (elapsed_ns, self.displayed_pose.clone()));
        self.previous_pose = None;
        self.dance_clue = clue;
    }

    pub(super) fn pose(&self, elapsed_ns: u64, now_ns: u64) -> (usize, &'static str) {
        if let (Some((dance, _)), Some(snapshot)) = (&self.lico, &self.dance_clue) {
            let time = now_ns.saturating_sub(snapshot.started_ns);
            let tick = (time.saturating_mul(u64::from(super::HZ)) / SECOND_NS) as usize;
            (tick, dance.phrase(time as f64 / SECOND_NS as f64))
        } else {
            let tick =
                (self.clock.motion_time(elapsed_ns) * u64::from(super::HZ) / SECOND_NS) as usize;
            let index = self.motion.frame(tick);
            (index, self.motion.phase(index))
        }
    }

    pub(super) fn submit(
        &mut self,
        surface: Ui4Surface,
        camera: RetainedCamera,
        frame: usize,
        elapsed_ns: u64,
        now_ns: u64,
        samples: u8,
    ) -> Result<(), i32> {
        let extent = match self.stage(camera, frame, elapsed_ns, now_ns) {
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
            samples,
        )
    }

    fn stage(
        &mut self,
        camera: RetainedCamera,
        frame: usize,
        elapsed_ns: u64,
        now_ns: u64,
    ) -> Result<[u32; 2]> {
        let frame_count = self.motion.frame_count();
        let dancing = self.clock.dancing;
        if self
            .transition
            .as_ref()
            .is_some_and(|(start, _)| elapsed_ns - *start >= MODE_BLEND_NS)
        {
            self.transition = None;
        }
        if !dancing
            && self.transition.is_none()
            && let Some(previous) = self.previous_pose
        {
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
        let dance_seconds = self
            .dance_clue
            .as_ref()
            .map(|snapshot| now_ns.saturating_sub(snapshot.started_ns) as f64 / SECOND_NS as f64);
        let stage = if let (Some((dance, _)), Some(seconds)) = (&self.lico, dance_seconds) {
            dance.pupil_stage(seconds)
        } else {
            self.motion.pupil_stage(frame)
        };
        let pupil_area = self.pupil.sample(stage, elapsed_ns);
        let pose = if let (Some((dance, _)), Some(seconds)) = (&mut self.lico, dance_seconds) {
            dance.sample(seconds)?
        } else {
            self.motion.sample(frame)?
        };
        let pose = if let Some((start, from)) = &self.transition {
            let t = (elapsed_ns - *start) as f32 / MODE_BLEND_NS as f32;
            self.blend_rig.blend_pose(
                from,
                pose.joint_world,
                pose.world_offset,
                t * t * (3.0 - 2.0 * t),
            );
            let head = self
                .blend_rig
                .head_from_bind()
                .context("missing transition head joint")?;
            let skin = self.blend_rig.skin();
            let lowest = self
                .source_vertices
                .iter()
                .map(|&index| skin.positions[index][1])
                .fold(f32::INFINITY, f32::min);
            let lift = (GROUND_Y - lowest).max(0.0);
            let world_offset = Vec3::Y * lift;
            let positions = if lift > 0.0 {
                self.lifted_positions.clear();
                self.lifted_positions
                    .extend(skin.positions.iter().map(|&[x, y, z]| [x, y + lift, z]));
                &self.lifted_positions
            } else {
                skin.positions
            };
            PlacedPose {
                positions,
                normals: skin.normals,
                head_from_bind: Mat4::from_translation(world_offset) * head,
                joint_world: skin.joint_world,
                world_offset,
            }
        } else {
            pose
        };
        self.displayed_pose
            .save(pose.joint_world, pose.world_offset);
        if (frame != 0 || dancing || self.transition.is_some()) && self.previous_pose != Some(frame)
        {
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
        for (column, start_row, source_width, pixels) in [
            (0, LIGHT_ROW, LIGHT_SIZE, self.light_pixels.as_slice()),
            (0, EYE_ROW, eye_detail::WIDTH, self.eye_detail.rgba()),
            (IRIS_COLUMN, IRIS_ROW, eye_sprite::SIZE, self.iris.rgba()),
        ] {
            for (row, colors) in pixels.chunks_exact(source_width * 4).enumerate() {
                let start = (start_row + row) * stride + column * 4;
                self.texture[start..start + colors.len()].copy_from_slice(colors);
            }
        }
        let swatches = [palette.swatch(0), palette.swatch(1)];
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
        for (position, [u, v]) in self.iris.vertices(pose.positions) {
            append(
                position,
                [
                    (u + IRIS_COLUMN as f32) / width as f32,
                    (v + IRIS_ROW as f32) / height as f32,
                ],
            );
        }
        let pupil_uv = palette.pupil();
        for position in self.iris.pupil_vertices(pose.positions, pupil_area) {
            append(position, pupil_uv);
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
        let t = if dancing || self.transition.is_some() {
            0.0
        } else {
            (frame.saturating_sub(drape_blend_start) as f32 / super::HZ as f32).clamp(0.0, 1.0)
        };
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
        let mut next_vertex = (self.source_vertices.len()
            + eye_sprite::VERTEX_COUNT as usize
            + eye_sprite::PUPIL_VERTEX_COUNT as usize
            + self.eye_detail.len()
            + self.drape.len() * PARTICLES * 2) as u32;
        self.indices.truncate(self.geometry_index_bytes);
        for (points, uv) in self.contacts.quads(0, GROUND_Y, &palette) {
            for (point, uv) in points.into_iter().zip(uv) {
                append(Vec3::from_array(point), uv);
            }
            self.indices.extend(
                [0, 1, 2, 2, 1, 3]
                    .into_iter()
                    .flat_map(|corner| (next_vertex + corner).to_le_bytes()),
            );
            next_vertex += 4;
        }
        let stroke = self.motion.stroke(frame);
        for &position in stroke.iter().flatten() {
            append(position, swatches[0]);
        }
        for _ in stroke.windows(2) {
            self.indices.extend(
                [0, 1, 2, 2, 1, 3]
                    .into_iter()
                    .flat_map(|corner| (next_vertex + corner).to_le_bytes()),
            );
            next_vertex += 2;
        }
        Ok([width, height])
    }
}

fn body_indices(mesh: &Mesh, body_start: usize) -> &[u32] {
    let skin = &mesh.indices[..mesh.skin_triangle_count * 3];
    &skin[skin.partition_point(|&index| (index as usize) < body_start)..]
}

fn frame_bounds(motion: &mut Motion, source: &[usize]) -> Result<(Vec3, [f32; 2])> {
    let is_heart = matches!(motion, Motion::Heart(_));
    let (right, back) = if is_heart {
        (-Vec3::X, -Vec3::Z)
    } else {
        (Vec3::Y.cross(camera_back(BASE_YAW)), view_back(BASE_YAW))
    };
    let up = back.cross(right);
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    let mut include = |position: Vec3| {
        let point = Vec3::new(right.dot(position), up.dot(position), back.dot(position));
        low = low.min(point);
        high = high.max(point);
    };
    for frame in (0..motion.frame_count()).step_by(5) {
        let pose = motion.sample(frame)?;
        for &index in source {
            include(Vec3::from_array(pose.positions[index]));
        }
    }
    if !is_heart {
        let (min, max) = Dance::framing_bounds();
        for x in [min[0], max[0]] {
            for y in [min[1], max[1]] {
                for z in [min[2], max[2]] {
                    include(Vec3::new(x, y, z));
                }
            }
        }
    }
    let center = (high + low) * 0.5;
    let half = (high - low) * 0.5;
    let arc_margin = half.z * ARC_LIMIT.sin();
    let span = [half.x + arc_margin + 0.04, half.y + 0.04];
    Ok((right * center.x + up * center.y + back * center.z, span))
}
