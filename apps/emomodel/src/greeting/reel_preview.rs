use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec3};
use trueos::vgpu::{Device, RetainedCamera, Ui4Surface};

use super::{
    SECOND_NS,
    body_tint::{self, BodyTint},
    butterfly::Butterfly,
    compact_mesh,
    dance::Dance,
    eye_detail::{self, EyeDetail},
    eye_sprite::{self, EyeSprite},
    fire_light::FireLight,
    floor, floor_material,
    foot_contact::FootProbe,
    hairband::{self, Hairband},
    hand_contact::HandProbe,
    lico_graph::{DanceClueSnapshot, Observer},
    live_butterfly::{self, Birth, FlightLight, LiveButterfly},
    pupil::{self, Stage},
    reel::{GROUND_Y, PlacedPose, Reel},
    reel_draw::{ReelDraw, VERTEX_BYTES},
    report,
    skin_study::{self, Lighting, Material},
    sky_light::{Key, SkyLight},
    sky_palette,
};
use crate::{
    EyeStyle,
    animation::{Animation, RigPose},
    autobs::{Bounds, Framing},
    hair::{Hair, PARTICLES},
    hair_body::BodyBack,
    hair_ribbon, light,
    mesh::Mesh,
    top::{self, Fit, Surface, TopStyle, finish},
};

const LIGHT_SIZE: usize = skin_study::SIZE;
const LIGHT_ROW: usize = floor_material::SWATCH_ROWS;
const IRIS_ROW: usize = LIGHT_ROW;
const IRIS_COLUMN: usize = LIGHT_SIZE + 1;
const TOP_COLUMN: usize = IRIS_COLUMN + eye_sprite::SIZE + 1;
const BODY_TINT_COLUMN: usize = TOP_COLUMN + light::DIFFUSE_WIDTH + 1;
const EYE_ROW: usize = IRIS_ROW + eye_sprite::SIZE;
const SKY_COLUMN: usize = eye_detail::WIDTH + 1;
const SPARK_COLUMN: usize = SKY_COLUMN + sky_palette::WIDTH;
const SPARK_SIZE: usize = 8;
const SPARK_EDGE: [Vec2; 8] = [
    Vec2::X,
    Vec2::new(0.24, 0.24),
    Vec2::Y,
    Vec2::new(-0.24, 0.24),
    Vec2::NEG_X,
    Vec2::new(-0.24, -0.24),
    Vec2::NEG_Y,
    Vec2::new(0.24, -0.24),
];
const SPARK_VERTICES: usize = 1 + SPARK_EDGE.len();
const SPARK_INDICES: usize = 3 * SPARK_EDGE.len();
const HAIRBAND_ROW: usize = EYE_ROW + eye_detail::HEIGHT;
const CONTACT_ROW: usize = HAIRBAND_ROW + hairband::HEIGHT;
const ATLAS_HEIGHT: usize = 512 + hairband::HEIGHT;
const MAX_ATLAS_WIDTH: usize = 1024;
const ATLAS_WIDTHS: [usize; 2] = [512, MAX_ATLAS_WIDTH];
const MODE_BLEND_NS: u64 = 400_000_000;
const BUTTERFLY_HOLD_NS: u64 = 800_000_000;
const BUTTERFLIES_PER_SKETCH: usize = 3;
// Reserve nine groups for consecutive sketches with overlapping flights.
const MAX_LIVE_BUTTERFLIES: usize = 9 * BUTTERFLIES_PER_SKETCH;
const SKETCH_OUTLINE_SLOT: usize = 3;
const LIVE_FILL_SLOT: usize = SKETCH_OUTLINE_SLOT + 1;
const LIVE_OUTLINE_SLOT: usize = LIVE_FILL_SLOT + MAX_LIVE_BUTTERFLIES;
const PELVIS_MARKER_SLOT: usize = LIVE_OUTLINE_SLOT + MAX_LIVE_BUTTERFLIES;
const PELVIS_MARKER_POINTS: [Vec2; 6] = [
    Vec2::new(-15.0, -4.0),
    Vec2::new(-9.0, 0.0),
    Vec2::new(-15.0, 4.0),
    Vec2::new(15.0, 4.0),
    Vec2::new(9.0, 0.0),
    Vec2::new(15.0, -4.0),
];
// The centerline vertices bracketing the normal crop in the embedded body.
const PELVIS_ANCHORS: [usize; 2] = [1063, 1071];
const LIVE_VERTICES: usize = MAX_LIVE_BUTTERFLIES * 3 * live_butterfly::MAX_TRIANGLES;
const FIREFLY_DEPTH_TRAVEL: f32 = 1.6;

const _: () = assert!(TOP_COLUMN + light::DIFFUSE_WIDTH <= ATLAS_WIDTHS[0]);
const _: () = assert!(LIGHT_ROW + top::COVERAGE_ROWS <= EYE_ROW);
const _: () = assert!(BODY_TINT_COLUMN + body_tint::WIDTH <= ATLAS_WIDTHS[0]);
const _: () = assert!(LIGHT_ROW + body_tint::HEIGHT <= EYE_ROW);
const _: () = assert!(SPARK_COLUMN + SPARK_SIZE <= ATLAS_WIDTHS[0]);
const _: () = assert!(EYE_ROW + SPARK_SIZE <= HAIRBAND_ROW);
const _: () = assert!(EYE_ROW + sky_palette::HEIGHT <= HAIRBAND_ROW);
const _: () = assert!(hairband::WIDTH <= ATLAS_WIDTHS[0]);
const _: () = assert!(finish::ATLAS_WIDTH <= ATLAS_WIDTHS[0]);
const _: () = assert!((PELVIS_MARKER_SLOT + 1) * 4 <= ATLAS_WIDTHS[0]);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FireflyPhase {
    Idle,
    Drawing,
    Following,
}

impl FireflyPhase {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Drawing => "drawing",
            Self::Following => "following",
        }
    }
}

#[derive(Clone, Copy)]
struct Firefly {
    position: Vec3,
    colors: [[u8; 3]; 2],
    phase: f32,
    gain: f32,
    mix: f32,
    state: FireflyPhase,
}

impl Firefly {
    fn flying(
        light: FlightLight,
        viewport: (u32, u32),
        camera: RetainedCamera,
        avatar_center: Vec3,
        back: Vec3,
    ) -> Self {
        let size = Vec2::new(viewport.0 as f32, viewport.1 as f32);
        let xy = 2.0 * light.center / size - Vec2::ONE;
        let projection = Mat4::from_cols_array(&camera.view_projection);
        let inverse = Mat4::from_cols_array(&camera.inverse_view_projection);
        let avatar_depth = projection.transform_point3(avatar_center).z;
        // The wings' clip depth is a layering value. Keep the projected light
        // just in front of the avatar and use recession for the distance fade.
        let position = inverse.transform_point3(xy.extend(avatar_depth))
            + back * (0.48 - 0.20 * light.recession);
        let distance = position.distance(avatar_center) + FIREFLY_DEPTH_TRAVEL * light.recession;
        let beyond = (distance - 0.65).max(0.0);
        let settle = (light.recession * 5.0).min(1.0);
        let gain = 1.0 - settle * 0.5 * beyond / (0.6 + beyond);
        Self {
            position,
            colors: light.colors,
            phase: light.phase,
            gain,
            mix: 1.0,
            state: FireflyPhase::Following,
        }
    }

    fn select(flight: Option<Self>, drawing: Option<Self>) -> Option<Self> {
        match (flight, drawing) {
            (Some(previous), Some(next)) => {
                // Interpolate one emitter when a new drawing takes over.
                let weight = next.mix;
                let colors = std::array::from_fn(|color| {
                    std::array::from_fn(|channel| {
                        let from = f32::from(previous.colors[color][channel]);
                        let to = f32::from(next.colors[color][channel]);
                        (from + (to - from) * weight).round() as u8
                    })
                });
                let phase_delta = (next.phase - previous.phase + 0.5).rem_euclid(1.0) - 0.5;
                Some(Self {
                    position: previous.position.lerp(next.position, weight),
                    colors,
                    phase: previous.phase + phase_delta * weight,
                    gain: previous.gain + (next.gain - previous.gain) * weight,
                    mix: 1.0,
                    state: next.state,
                })
            }
            (flight, drawing) => drawing.or(flight),
        }
    }
}

fn roll_butterfly() -> u32 {
    trueos::rng::u64() as u32
}

pub(super) enum Motion {
    Reel(Reel),
    Nod(Animation),
    Butterfly {
        variants: Box<[Butterfly; 2]>,
        artwork: usize,
    },
}

impl Motion {
    pub(super) fn butterflies(glb: &[u8], body_end: usize) -> Result<Self> {
        Ok(Self::Butterfly {
            variants: Box::new([
                Butterfly::new(glb, &super::MATERIALS, body_end, 0)?,
                Butterfly::new(glb, &super::MATERIALS, body_end, 1)?,
            ]),
            artwork: 0,
        })
    }

    fn select_butterfly(&mut self, seed: u32) {
        if let Self::Butterfly { artwork, .. } = self {
            *artwork = live_butterfly::artwork(seed);
        }
    }

    fn stroke_capacity(&self) -> (usize, usize) {
        let Self::Butterfly { variants, .. } = self else {
            return (0, 0);
        };
        variants
            .iter()
            .fold((0, 0), |(max_pairs, max_indices), butterfly| {
                let (pairs, indices) = butterfly.strokes(butterfly.frame_count() - 1).fold(
                    (0, 0),
                    |(pairs, indices), stroke| {
                        (
                            pairs + stroke.len(),
                            indices + stroke.len().saturating_sub(1) * 6,
                        )
                    },
                );
                (max_pairs.max(pairs), max_indices.max(indices))
            })
    }

    fn frame_count(&self) -> usize {
        match self {
            Self::Reel(reel) => reel.frame_count(),
            Self::Nod(nod) => (nod.duration() * super::HZ as f32).round() as usize + 1,
            Self::Butterfly { variants, artwork } => variants[*artwork].frame_count(),
        }
    }

    fn frame(&self, tick: usize) -> usize {
        if matches!(self, Self::Butterfly { .. }) {
            tick.min(self.frame_count() - 1)
        } else {
            tick % self.frame_count()
        }
    }

    fn phase(&self, frame: usize) -> &'static str {
        match self {
            Self::Reel(reel) => reel.phase(frame),
            Self::Nod(_) => "Nod",
            Self::Butterfly { variants, artwork } => variants[*artwork].phase(frame),
        }
    }

    fn pupil_stage(&self, frame: usize) -> Stage {
        match self {
            Self::Reel(reel) => reel.pupil_stage(frame),
            Self::Nod(nod) => pupil::nod_stage(frame as f32 / super::HZ as f32, nod.duration()),
            Self::Butterfly { variants, artwork } => variants[*artwork].pupil_stage(frame),
        }
    }

    fn strokes(&self, frame: usize) -> impl Iterator<Item = &[[Vec3; 2]]> {
        let butterfly = match self {
            Self::Butterfly { variants, artwork } => Some(&variants[*artwork]),
            _ => None,
        };
        butterfly
            .into_iter()
            .flat_map(move |butterfly| butterfly.strokes(frame))
    }

    fn birth(&self, frame: usize, camera: RetainedCamera, viewport: (u32, u32)) -> Option<Birth> {
        match self {
            Self::Butterfly { variants, artwork } => variants[*artwork].birth(
                frame,
                Mat4::from_cols_array(&camera.view_projection),
                viewport,
            ),
            _ => None,
        }
    }

    fn drawing_light(&self, frame: usize) -> Option<(Vec3, f32)> {
        match self {
            Self::Butterfly { variants, artwork } => variants[*artwork].drawing_light(frame),
            _ => None,
        }
    }

    fn ink_tip(&self, frame: usize) -> Option<Vec3> {
        match self {
            Self::Butterfly { variants, artwork } => variants[*artwork].ink_tip(frame),
            _ => None,
        }
    }

    fn sample(&mut self, frame: usize) -> Result<PlacedPose<'_>> {
        match self {
            Self::Reel(reel) => reel.sample(frame),
            Self::Butterfly { variants, artwork } => variants[*artwork].sample(frame),
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
            Self::Butterfly { variants, .. } => variants
                .iter_mut()
                .try_for_each(|butterfly| butterfly.apply_bind_normals(section, normals)),
        }
    }
}

#[derive(Default)]
struct PlaybackClock {
    changed_ns: u64,
    reel_elapsed_ns: u64,
    paused: bool,
}

impl PlaybackClock {
    fn select(&mut self, paused: bool, elapsed_ns: u64) {
        if self.paused == paused {
            return;
        }
        if paused {
            self.reel_elapsed_ns = self.motion_time(elapsed_ns);
        }
        self.changed_ns = elapsed_ns;
        self.paused = paused;
    }

    fn motion_time(&self, elapsed_ns: u64) -> u64 {
        if self.paused {
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
            Motion::Butterfly { variants, artwork } => {
                materials.extend(variants[*artwork].bind_positions(section).map(Material::at))
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
    butterfly: Option<Motion>,
    motion_framing: Framing,
    dance_framing: Framing,
    butterfly_started_ns: Option<u64>,
    sketch_seed: Option<u32>,
    first_butterfly_spawned: bool,
    live_butterflies: Vec<LiveButterfly>,
    firefly_owner: Option<u64>,
    firefly_phase: FireflyPhase,
    lico: Option<(Dance, Observer)>,
    dance_clue: Option<Arc<DanceClueSnapshot>>,
    clock: PlaybackClock,
    blend_rig: Animation,
    displayed_pose: RigPose,
    transition: Option<(u64, RigPose)>,
    lifted_positions: Vec<[f32; 3]>,
    draw: ReelDraw,
    source_vertices: Vec<usize>,
    body_tint: BodyTint,
    material_slots: Vec<Option<usize>>,
    hairband: Hairband,
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
    pelvis_markers: bool,
    sky_palette: sky_palette::Palette,
    top_style: TopStyle,
    top_fit: Vec<Fit>,
    top_rows: Vec<f32>,
    top_pixels: Vec<u8>,
    top_ramps: [Vec<u8>; Surface::ALL.len()],
    top_finish: finish::Finish,
    vertices: Vec<u8>,
    indices: Vec<u8>,
    geometry_index_bytes: usize,
    geometry_vertices: u32,
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
    ) -> Result<Self> {
        let is_butterfly = matches!(&motion, Motion::Butterfly { .. });
        let sketch_seed = is_butterfly.then(roll_butterfly);
        if let Some(seed) = sketch_seed {
            motion.select_butterfly(seed);
        }
        let mut butterfly = (!is_butterfly)
            .then(|| Motion::butterflies(glb, mesh.material_vertices[1].end))
            .transpose()?;
        let mut dance = (!is_butterfly)
            .then(|| Dance::new(glb, &super::MATERIALS))
            .transpose()?;
        let mut blend_rig = Animation::procedural(glb, &super::MATERIALS)?;
        super::apply_normal_maps(glb, !studying_skin, |section, normals| {
            motion.apply_bind_normals(section, normals)?;
            if let Some(butterfly) = &mut butterfly {
                butterfly.apply_bind_normals(section, normals)?;
            }
            if let Some(dance) = &mut dance {
                dance.apply_bind_normals(section, normals)?;
            }
            blend_rig.apply_bind_normals(section, normals)
        })
        .await?;
        let body_start = mesh.material_vertices[1].start;
        let feet = FootProbe::new(glb, body_start)?;
        let hands = HandProbe::new(glb, body_start)?;
        let eye_detail = EyeDetail::load(glb).await?;
        let iris = EyeSprite::load(eyes).await?;
        ensure!(
            eye_detail.rgba().len() == eye_detail::WIDTH * eye_detail::HEIGHT * 4,
            "wrong reel eye detail extent"
        );
        let body_tint = BodyTint::load(glb, mesh).await?;
        let hairband = Hairband::load(glb, mesh).await?;
        let (source_vertices, mut compact_indices) = compact_mesh(mesh);
        body_tint.remap_indices(&mut compact_indices, u32::try_from(source_vertices.len())?);
        let eye_base = source_vertices.len() + body_tint.vertex_count();
        let pupil_base = eye_base + eye_sprite::VERTEX_COUNT as usize;
        let detail_base = pupil_base + eye_sprite::PUPIL_VERTEX_COUNT as usize;
        let hairband_base = detail_base + eye_detail.len();
        let hair_base = hairband_base + hairband.len();
        let strand_count = mesh.strand_guides.len();
        let hair_vertices = strand_count * PARTICLES * 2;
        let skin_materials = studying_skin
            .then(|| skin_materials(&motion, mesh))
            .transpose()?;
        let top_fit: Vec<_> = blend_rig.bind_positions(1).map(Fit::at).collect();
        ensure!(
            top_fit.len() == mesh.material_vertices[1].len(),
            "top fit does not match the body mesh"
        );
        let gesture = butterfly.as_ref().unwrap_or(&motion);
        let (stroke_pairs, stroke_indices) = gesture.stroke_capacity();
        let geometry_vertices = u32::try_from(hair_base + hair_vertices)?;
        let trim_vertices = mesh.material_triangles[1].len() * 3;
        let vertex_count = geometry_vertices as usize
            + trim_vertices
            + 4 * floor::CONTACTS
            + 2 * stroke_pairs
            + SPARK_VERTICES
            + LIVE_VERTICES
            + PELVIS_MARKER_POINTS.len();
        let transient_indices = floor::CONTACTS * 6
            + stroke_indices
            + SPARK_INDICES
            + LIVE_VERTICES
            + PELVIS_MARKER_POINTS.len();
        let mut indices = Vec::with_capacity(
            (compact_indices.len()
                + eye_sprite::INDEX_COUNT
                + eye_sprite::PUPIL_INDEX_COUNT
                + eye_detail.indices().len()
                + hairband.indices().len()
                + strand_count * PARTICLES * 6
                + transient_indices)
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
        let hairband_base = u32::try_from(hairband_base)?;
        indices.extend(
            hairband
                .indices()
                .iter()
                .flat_map(|&index| (hairband_base + index).to_le_bytes()),
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
            geometry_index_bytes + transient_indices * size_of::<u32>(),
            4 * MAX_ATLAS_WIDTH * (ATLAS_HEIGHT + finish::MAX_ROWS),
        )?;
        if studying_skin {
            report(format_args!(
                "skin study ready sample=vertex lookup=64x32 roughness=100%"
            ));
        }

        let motion_framing = frame_motion(&mut motion, &source_vertices, &hairband)?;
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
            Motion::Butterfly { .. } => ("Butterfly", "gesture"),
        };
        report(format_args!(
            "avatar ready target_hz={} clips={clips} poses={} playback={playback}",
            super::HZ,
            motion.frame_count()
        ));
        Ok(Self {
            mesh,
            motion,
            butterfly,
            motion_framing,
            dance_framing: dance_framing(),
            butterfly_started_ns: is_butterfly.then_some(0),
            sketch_seed,
            first_butterfly_spawned: false,
            live_butterflies: Vec::with_capacity(MAX_LIVE_BUTTERFLIES),
            firefly_owner: None,
            firefly_phase: FireflyPhase::Idle,
            lico: dance.map(|dance| (dance, Observer::new())),
            dance_clue: None,
            clock: PlaybackClock::default(),
            blend_rig,
            displayed_pose: RigPose::default(),
            transition: None,
            lifted_positions: Vec::new(),
            draw,
            source_vertices,
            body_tint,
            material_slots,
            hairband,
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
            pelvis_markers: studying_skin && !is_butterfly,
            sky_palette: sky_palette::Palette::new(Key::Neutral),
            top_style: TopStyle::default(),
            top_fit,
            top_rows: Vec::new(),
            top_pixels: Vec::new(),
            top_ramps: Default::default(),
            top_finish: finish::Finish::default(),
            vertices: Vec::with_capacity(vertex_count * VERTEX_BYTES),
            indices,
            geometry_index_bytes,
            geometry_vertices,
            texture: Vec::new(),
        })
    }

    pub(super) fn toggle_pelvis_markers(&mut self) {
        if self.skin_materials.is_some() {
            self.pelvis_markers = !self.pelvis_markers;
        }
    }

    pub(super) fn set_top(&mut self, style: TopStyle) -> Result<()> {
        if style == self.top_style {
            return Ok(());
        }
        let fit = |style| TopStyle {
            color: None,
            ..style
        };
        let fit_changed = self.top_style.color.is_some() != style.color.is_some()
            || fit(self.top_style) != fit(style);
        if fit_changed {
            let mut trim = if style.color.is_some() {
                finish::Finish::build(
                    style,
                    &self.top_fit,
                    self.body_start,
                    self.mesh.material_triangles[1].clone(),
                    self.mesh.indices.as_chunks::<3>().0,
                    |triangle| self.body_tint.triangle_brightness(triangle),
                )
                .context("shirt trim atlas exceeds its extent")?
            } else {
                finish::Finish::default()
            };
            self.top_finish.restore_indices(&mut self.indices);
            trim.replace_indices(&mut self.indices, self.geometry_vertices);
            self.top_finish = trim;
            self.top_rows.clear();
            if style.color.is_some() {
                self.top_rows
                    .extend(self.top_fit.iter().map(|fit| fit.coverage_row(style)));
            }
        }
        if style.color != self.top_style.color {
            self.top_pixels.clear();
            if let Some(color) = style.color {
                let lighting_ramp = if self.skin_materials.is_some() {
                    skin_study::color_lookup_rgba
                } else {
                    light::diffuse_lookup_rgba
                };
                self.top_ramps = Surface::ALL.map(|surface| lighting_ramp(surface.albedo(color)));
                for row in 0..top::COVERAGE_ROWS {
                    self.top_pixels
                        .extend_from_slice(&self.top_ramps[Surface::at_row(row) as usize]);
                }
            }
        }
        self.top_style = style;
        Ok(())
    }

    pub(super) fn framing(&self) -> Framing {
        self.motion_framing
    }

    pub(super) fn sync_motion(&mut self, elapsed_ns: u64, butterfly_requested: bool) -> Framing {
        if butterfly_requested
            && self.sketch_seed.is_none()
            && (self.butterfly.is_some() || matches!(self.motion, Motion::Butterfly { .. }))
        {
            let seed = roll_butterfly();
            self.butterfly
                .as_mut()
                .unwrap_or(&mut self.motion)
                .select_butterfly(seed);
            self.butterfly_started_ns = Some(elapsed_ns);
            self.sketch_seed = Some(seed);
            self.clock.select(true, elapsed_ns);
            self.blend_from_displayed(elapsed_ns);
            report(format_args!("avatar mode=butterfly source=key-3"));
            return self.current_framing();
        }
        if let (Some(butterfly), Some(start)) = (&self.butterfly, self.butterfly_started_ns) {
            let duration = (butterfly.frame_count() - 1) as u64 * SECOND_NS / u64::from(super::HZ);
            if elapsed_ns - start < duration + BUTTERFLY_HOLD_NS || self.sketch_seed.is_some() {
                return self.current_framing();
            }
            self.butterfly_started_ns = None;
            self.clock.select(self.dance_clue.is_some(), elapsed_ns);
            self.blend_from_displayed(elapsed_ns);
            report(format_args!("avatar gesture=butterfly-complete"));
        }
        self.sync_lico(elapsed_ns);
        self.current_framing()
    }

    fn current_framing(&self) -> Framing {
        if self.dance_clue.is_some() {
            self.dance_framing
        } else {
            self.motion_framing
        }
    }

    /// Returns true when the first completed sketch comes alive.
    pub(super) fn on_published(
        &mut self,
        frame: usize,
        now_ns: u64,
        camera: RetainedCamera,
        viewport: (u32, u32),
    ) -> bool {
        let Some(seed) = self.sketch_seed else {
            return false;
        };
        let motion = if self.butterfly_started_ns.is_some() {
            self.butterfly.as_ref().unwrap_or(&self.motion)
        } else {
            &self.motion
        };
        let Some(birth) = motion.birth(frame, camera, viewport) else {
            return false;
        };
        self.sketch_seed = None;
        self.live_butterflies
            .retain(|butterfly| !butterfly.has_departed(now_ns));
        let excess = (self.live_butterflies.len() + BUTTERFLIES_PER_SKETCH)
            .saturating_sub(MAX_LIVE_BUTTERFLIES);
        self.live_butterflies.drain(..excess);
        self.live_butterflies.extend(
            (0..BUTTERFLIES_PER_SKETCH).map(|lane| LiveButterfly::new(seed, now_ns, birth, lane)),
        );
        self.firefly_owner = Some(now_ns);
        report(format_args!(
            "avatar butterfly spawned count={} group={BUTTERFLIES_PER_SKETCH} tier=1 artwork={}",
            self.live_butterflies.len(),
            live_butterfly::artwork(seed),
        ));
        !std::mem::replace(&mut self.first_butterfly_spawned, true)
    }

    pub(super) fn firefly_phase(&self) -> FireflyPhase {
        self.firefly_phase
    }

    fn blend_from_displayed(&mut self, elapsed_ns: u64) {
        self.transition = (!self.displayed_pose.joints.is_empty())
            .then(|| (elapsed_ns, self.displayed_pose.clone()));
        self.previous_pose = None;
    }

    fn sync_lico(&mut self, elapsed_ns: u64) {
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
        self.blend_from_displayed(elapsed_ns);
        self.dance_clue = clue;
    }

    pub(super) fn pose(&self, elapsed_ns: u64, now_ns: u64) -> (usize, &'static str) {
        if let Some(start) = self.butterfly_started_ns {
            let butterfly = self.butterfly.as_ref().unwrap_or(&self.motion);
            let tick = ((elapsed_ns - start) * u64::from(super::HZ) / SECOND_NS) as usize;
            let index = butterfly.frame(tick);
            (index, butterfly.phase(index))
        } else if let (Some((dance, _)), Some(snapshot)) = (&self.lico, &self.dance_clue) {
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
        sky: &SkyLight,
    ) -> Result<(), i32> {
        let info = surface.info();
        let extent = match self.stage(
            camera,
            (info.width, info.height),
            frame,
            elapsed_ns,
            now_ns,
            sky,
        ) {
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
        viewport: (u32, u32),
        frame: usize,
        elapsed_ns: u64,
        now_ns: u64,
        sky: &SkyLight,
    ) -> Result<[u32; 2]> {
        let butterfly = self.butterfly_started_ns.is_some();
        let motion = match (&mut self.butterfly, butterfly) {
            (Some(butterfly), true) => butterfly,
            _ => &mut self.motion,
        };
        let frame_count = motion.frame_count();
        let dance_clue = self.dance_clue.as_ref().filter(|_| !butterfly);
        let dancing = dance_clue.is_some();
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
                    let pose = motion.sample(skipped)?;
                    self.body
                        .update(pose.positions, body_indices(self.mesh, self.body_start));
                    self.hair.step(pose.head_from_bind, Vec3::ZERO, &self.body);
                }
            }
        }
        let dance_seconds = dance_clue
            .map(|snapshot| now_ns.saturating_sub(snapshot.started_ns) as f64 / SECOND_NS as f64);
        let stage = if let (Some((dance, _)), Some(seconds)) = (&self.lico, dance_seconds) {
            dance.pupil_stage(seconds)
        } else {
            motion.pupil_stage(frame)
        };
        let pupil_area = self.pupil.sample(stage, elapsed_ns);
        let ink_color = self.sketch_seed.map(live_butterfly::outline_color);
        let spark = ink_color.and_then(|color| motion.ink_tip(frame).map(|tip| (tip, color)));
        let drawing = self.sketch_seed.and_then(|seed| {
            motion.drawing_light(frame).and_then(|(position, mix)| {
                (mix > 0.0).then(|| Firefly {
                    position,
                    colors: live_butterfly::light_colors(seed),
                    phase: frame as f32 / super::HZ as f32 * 0.38 + (seed & 7) as f32 / 8.0,
                    gain: 1.0,
                    mix,
                    state: FireflyPhase::Drawing,
                })
            })
        });
        let pose = if let (Some((dance, _)), Some(seconds)) = (&mut self.lico, dance_seconds) {
            dance.sample(seconds)?
        } else {
            motion.sample(frame)?
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
        self.live_butterflies
            .retain(|butterfly| !butterfly.has_departed(now_ns));
        let back = super::camera_view_back(camera);
        let pelvis_marker = if self.pelvis_markers {
            let [upper, lower] = PELVIS_ANCHORS.map(|index| self.body_start + index);
            (Vec3::from_array(pose.normals[upper]).dot(back) > 0.0).then(|| {
                (Vec3::from_array(pose.positions[upper]) + Vec3::from_array(pose.positions[lower]))
                    * 0.5
            })
        } else {
            None
        };
        let fire = if sky.firefly {
            let avatar_center =
                (left_heel.0 + left_toe.0 + right_heel.0 + right_toe.0) * 0.25 + 0.85 * Vec3::Y;
            let flight = self
                .live_butterflies
                .iter()
                .rev()
                .find(|butterfly| {
                    butterfly.is_lead() && Some(butterfly.started_ns()) == self.firefly_owner
                })
                .and_then(|butterfly| butterfly.light(viewport, now_ns))
                .map(|light| Firefly::flying(light, viewport, camera, avatar_center, back));
            Firefly::select(flight, drawing)
        } else {
            None
        };
        self.firefly_phase = fire.map_or(FireflyPhase::Idle, |light| light.state);
        let emitter = fire.map(|light| {
            let [ember, flare] = light.colors;
            FireLight::new(light.position, back, light.phase, ember, flare)
        });
        self.contacts.clear();
        self.contacts.push(
            GROUND_Y,
            [left_heel, left_toe, right_heel, right_toe],
            self.hands.sample(pose.positions),
        );
        let palette = self.contacts.bake_rows(
            &ATLAS_WIDTHS,
            ATLAS_HEIGHT + self.top_finish.rows,
            CONTACT_ROW + self.top_finish.rows,
        )?;
        let [width, height] = palette.extent();
        palette.write_rgba(&mut self.texture);
        let stride = width as usize * 4;
        self.sky_palette
            .update(sky.key, fire.map(|light| light.colors));
        let skin_palette = &self.sky_palette;
        let top_width = if self.skin_materials.is_some() {
            LIGHT_SIZE
        } else {
            light::DIFFUSE_WIDTH
        };
        for (column, start_row, source_width, pixels) in [
            (0, EYE_ROW, eye_detail::WIDTH, self.eye_detail.rgba()),
            (IRIS_COLUMN, IRIS_ROW, eye_sprite::SIZE, self.iris.rgba()),
            (TOP_COLUMN, LIGHT_ROW, top_width, self.top_pixels.as_slice()),
            (SKY_COLUMN, EYE_ROW, sky_palette::WIDTH, &skin_palette.rgba),
            (0, HAIRBAND_ROW, hairband::WIDTH, self.hairband.rgba()),
        ] {
            for (row, colors) in pixels.chunks_exact(source_width * 4).enumerate() {
                let start = (start_row + row) * stride + column * 4;
                self.texture[start..start + colors.len()].copy_from_slice(colors);
            }
        }
        if let Some(color) = ink_color {
            set_swatch(&mut self.texture, stride, SKETCH_OUTLINE_SLOT, color);
        }
        if pelvis_marker.is_some() {
            set_swatch(
                &mut self.texture,
                stride,
                PELVIS_MARKER_SLOT,
                [21, 79, 166, 255],
            );
        }
        if let Some((_, color)) = spark {
            paint_spark_tile(&mut self.texture, stride, color);
        }
        for (index, butterfly) in self.live_butterflies.iter().enumerate() {
            set_swatch(
                &mut self.texture,
                stride,
                LIVE_FILL_SLOT + index,
                butterfly.fill(),
            );
            set_swatch(
                &mut self.texture,
                stride,
                LIVE_OUTLINE_SLOT + index,
                butterfly.outline(),
            );
        }
        let swatches = [palette.swatch(0), palette.swatch(1)];
        let view_projection = Mat4::from_cols_array(&camera.view_projection);
        let diffuse_direction = sky.key_direction();
        let skin = self
            .skin_materials
            .as_deref()
            .map(|materials| (materials, Lighting::with_key(diffuse_direction, back)));
        let top_light = |index: usize, normal: Vec3| {
            let existing = if let Some((materials, lighting)) = &skin {
                lighting.coordinate(normal, materials[index]) * LIGHT_SIZE as f32
            } else {
                let diffuse = normal.dot(diffuse_direction).clamp(-1.0, 1.0);
                (diffuse + 1.0) * (light::DIFFUSE_WIDTH / 2) as f32 + 0.5
            };
            if let Some((fire, emitter)) = fire.zip(emitter.as_ref()) {
                let position = Vec3::from_array(pose.positions[index]);
                let irradiance = emitter.at(position).direct(normal, Vec3::ONE);
                let energy = sky_palette::luminance(irradiance) * fire.gain;
                let column = if skin.is_some() {
                    skin_study::energy_coordinate(skin_study::AMBIENT + energy) * LIGHT_SIZE as f32
                } else {
                    let diffuse = (energy / (1.0 - light::AMBIENT)).clamp(0.0, 1.0);
                    (diffuse + 1.0) * (light::DIFFUSE_WIDTH / 2) as f32 + 0.5
                };
                existing + (column - existing) * fire.mix
            } else {
                existing
            }
        };
        if self.top_finish.rows != 0 {
            self.top_finish.write(
                &mut self.texture[CONTACT_ROW * stride..],
                width as usize,
                &self.top_ramps,
                pose.positions,
                pose.normals,
                top_light,
            );
        }
        self.vertices.clear();
        let output = &mut self.vertices;
        let mut append = |position: Vec3, uv: [f32; 2]| {
            append_clip_vertex(output, view_projection.transform_point3(position), uv);
        };
        let shade_uv = |index: usize| {
            if let Some(slot) = self.material_slots[index] {
                swatches[slot]
            } else {
                let normal = Vec3::from_array(pose.normals[index]);
                let top_row = index
                    .checked_sub(self.body_start)
                    .and_then(|body_index| self.top_rows.get(body_index));
                let (column, row) = if let Some(&row) = top_row {
                    (
                        TOP_COLUMN as f32 + top_light(index, normal),
                        LIGHT_ROW as f32 + row,
                    )
                } else {
                    let ambient = if skin.is_some() {
                        skin_study::AMBIENT
                    } else {
                        light::AMBIENT
                    };
                    let base = skin_palette.base;
                    let key = || {
                        let energy = if let Some((materials, lighting)) = &skin {
                            skin_study::light_rgb(lighting.energy(normal, materials[index]))
                        } else {
                            Vec3::splat(light::diffuse_energy(normal, diffuse_direction))
                        };
                        sky.key_light(base, base * energy, ambient)
                    };
                    let lit = match fire.zip(emitter.as_ref()) {
                        Some((fire, emitter)) => {
                            let position = Vec3::from_array(pose.positions[index]);
                            let light = base * ambient
                                + emitter.at(position).direct(normal, base) * fire.gain;
                            if fire.mix < 1.0 {
                                key().lerp(light, fire.mix)
                            } else {
                                light
                            }
                        }
                        None => key(),
                    };
                    let color = sky.environment(normal, base, lit);
                    let [u, v] = skin_palette.coordinates(color);
                    (SKY_COLUMN as f32 + u, EYE_ROW as f32 + v)
                };
                [column / width as f32, row / height as f32]
            }
        };
        for &index in &self.source_vertices {
            append(Vec3::from_array(pose.positions[index]), shade_uv(index));
        }
        self.body_tint.write_tiles(
            &mut self.texture,
            [width as usize, height as usize],
            [BODY_TINT_COLUMN, LIGHT_ROW],
            (!self.top_rows.is_empty()).then_some(LIGHT_ROW),
            shade_uv,
        );
        let tile_origin = Vec2::new(BODY_TINT_COLUMN as f32, LIGHT_ROW as f32);
        let dimensions = Vec2::new(width as f32, height as f32);
        for (index, uv) in self.body_tint.vertices() {
            append(
                Vec3::from_array(pose.positions[index]),
                ((uv + tile_origin) / dimensions).to_array(),
            );
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
        for (position, [u, v]) in self.hairband.vertices(pose.positions) {
            let uv = [
                u * hairband::WIDTH as f32 / width as f32,
                (HAIRBAND_ROW as f32 + v * hairband::HEIGHT as f32) / height as f32,
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
        for (index, uv) in self.top_finish.vertices() {
            let uv = (uv + Vec2::Y * CONTACT_ROW as f32) / dimensions;
            append(Vec3::from_array(pose.positions[index]), uv.to_array());
        }
        let mut next_vertex = self.geometry_vertices + self.top_finish.vertex_count() as u32;
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
        let sketch_outline = palette.swatch(SKETCH_OUTLINE_SLOT);
        for stroke in motion.strokes(frame).filter(|_| self.sketch_seed.is_some()) {
            for &position in stroke.iter().flatten() {
                append(position, sketch_outline);
            }
            for (i, _) in stroke.windows(2).enumerate() {
                let base = next_vertex + 2 * i as u32;
                self.indices.extend(
                    [0, 1, 2, 2, 1, 3]
                        .into_iter()
                        .flat_map(|corner| (base + corner).to_le_bytes()),
                );
            }
            next_vertex += 2 * stroke.len() as u32;
        }
        for (index, butterfly) in self.live_butterflies.iter().enumerate() {
            let fill = palette.swatch(LIVE_FILL_SLOT + index);
            let outline = palette.swatch(LIVE_OUTLINE_SLOT + index);
            butterfly.triangles(viewport, now_ns, |triangle, is_outline| {
                let uv = if is_outline { outline } else { fill };
                for vertex in triangle {
                    append_clip_vertex(output, vertex, uv);
                    self.indices.extend_from_slice(&next_vertex.to_le_bytes());
                    next_vertex += 1;
                }
            });
        }
        if let Some(center) = pelvis_marker {
            let mut clip = view_projection.transform_point3(center);
            clip.z = (clip.z - 0.008).max(0.0);
            let pixel_to_clip = Vec2::new(2.0 / viewport.0 as f32, 2.0 / viewport.1 as f32);
            let uv = palette.swatch(PELVIS_MARKER_SLOT);
            for point in PELVIS_MARKER_POINTS {
                append_clip_vertex(output, clip + (point * pixel_to_clip).extend(0.0), uv);
                self.indices.extend_from_slice(&next_vertex.to_le_bytes());
                next_vertex += 1;
            }
        }
        if let Some((tip, _)) = spark {
            let mut center = view_projection.transform_point3(tip);
            center.z = (center.z - 0.001).max(0.0);
            let pixel_to_clip = Vec2::new(2.0 / viewport.0 as f32, 2.0 / viewport.1 as f32);
            let t = frame as f32;
            let twinkle = 0.5 + 0.5 * (1.53 * t).sin();
            let radius = 2.5 + 1.5 * twinkle;
            let rotation = Vec2::from_angle(0.17 * t);
            let uv_min = Vec2::from_array(palette.texel_uv(SPARK_COLUMN, EYE_ROW));
            let uv_max = Vec2::from_array(
                palette.texel_uv(SPARK_COLUMN + SPARK_SIZE - 1, EYE_ROW + SPARK_SIZE - 1),
            );
            let uv_center = (uv_min + uv_max) * 0.5;
            let uv_radius = (uv_max - uv_min) * 0.5;
            append_clip_vertex(output, center, uv_center.to_array());
            // Clip the stamp to its opaque silhouette so it can share the depth draw.
            for (i, unit) in SPARK_EDGE.into_iter().enumerate() {
                let offset = rotation.rotate(unit) * radius * pixel_to_clip;
                append_clip_vertex(
                    output,
                    center + offset.extend(0.0),
                    (uv_center + unit * uv_radius).to_array(),
                );
                let left = next_vertex + 1 + i as u32;
                let right = next_vertex + 1 + ((i + 1) % SPARK_EDGE.len()) as u32;
                self.indices.extend(
                    [next_vertex, left, right]
                        .into_iter()
                        .flat_map(u32::to_le_bytes),
                );
            }
        }
        Ok([width, height])
    }
}

fn append_clip_vertex(vertices: &mut Vec<u8>, position: Vec3, uv: [f32; 2]) {
    for value in position.to_array().into_iter().chain(uv) {
        vertices.extend_from_slice(&value.to_le_bytes());
    }
}

fn paint_spark_tile(texture: &mut [u8], stride: usize, color: [u8; 4]) {
    for row in 0..SPARK_SIZE {
        let start = (EYE_ROW + row) * stride + 4 * SPARK_COLUMN;
        let texels = &mut texture[start..start + 4 * SPARK_SIZE];
        for texel in texels.chunks_exact_mut(4) {
            texel.copy_from_slice(&color);
        }
        if (3..5).contains(&row) {
            texels[4 * 3..4 * 5].fill(255);
        }
    }
}

fn set_swatch(texture: &mut [u8], stride: usize, slot: usize, [r, g, b, a]: [u8; 4]) {
    let premultiply = |channel| ((u16::from(channel) * u16::from(a) + 127) / 255) as u8;
    let rgba = [premultiply(r), premultiply(g), premultiply(b), a];
    for y in 0..floor_material::SWATCH_ROWS {
        for x in slot * 4..(slot + 1) * 4 {
            let start = y * stride + 4 * x;
            texture[start..start + 4].copy_from_slice(&rgba);
        }
    }
}

fn body_indices(mesh: &Mesh, body_start: usize) -> &[u32] {
    let skin = &mesh.indices[..mesh.skin_triangle_count * 3];
    &skin[skin.partition_point(|&index| (index as usize) < body_start)..]
}

fn frame_motion(motion: &mut Motion, source: &[usize], hairband: &Hairband) -> Result<Framing> {
    let mut bounds = Bounds::new(matches!(motion, Motion::Butterfly { .. }));
    let include_pose = |bounds: &mut Bounds, pose: PlacedPose<'_>| {
        for &index in source {
            bounds.include(Vec3::from_array(pose.positions[index]));
        }
        for (position, _) in hairband.vertices(pose.positions) {
            bounds.include(Vec3::from_array(position));
        }
    };
    if let Motion::Butterfly { variants, .. } = motion {
        for butterfly in variants.iter_mut() {
            let last = butterfly.frame_count() - 1;
            for frame in (0..last).step_by(5).chain([last]) {
                include_pose(&mut bounds, butterfly.sample(frame)?);
            }
            for &point in butterfly.strokes(last).flatten().flatten() {
                bounds.include(point);
            }
        }
    } else {
        let last = motion.frame_count() - 1;
        for frame in (0..last).step_by(5).chain([last]) {
            include_pose(&mut bounds, motion.sample(frame)?);
        }
    }
    // Keep a small reserve for inter-sample extrema and the live hair solver.
    Ok(bounds.framing(0.01))
}

fn dance_framing() -> Framing {
    let mut bounds = Bounds::new(false);
    let (min, max) = Dance::framing_bounds();
    for x in [min[0], max[0]] {
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                bounds.include(Vec3::new(x, y, z));
            }
        }
    }
    bounds.framing(0.02)
}
