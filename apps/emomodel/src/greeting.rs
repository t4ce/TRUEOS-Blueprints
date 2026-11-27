use anyhow::{Context, Result, ensure};
use bfly::backdrop::Backdrop;
use glam::{Vec2, Vec3};
use std::collections::VecDeque;
use std::ops::Range;
use std::path::Path;
use trueos::{
    clock, input, logl,
    ui4_scene::{Damage, Error, Frame},
    vgpu::{
        self, Buffer, Capabilities, Device, Queue, QueueClass, RetainedCamera, RetainedDrawRange,
        RetainedFrameSubmit, RetainedMesh, RetainedMeshDescriptor, RetainedTexturedFrameV1,
        RetainedTransformSeed, Ui4Surface,
    },
    vmedia::{self, ImageFormat, RetainedTexture},
    vsys,
};

use crate::{
    EyeStyle,
    animation::{ARM_RECOVERY_SECONDS, Animation, SAMPLE_HZ as HZ},
    autobs::{Autobs, BASE_YAW, Bounds, Framing, ObserverPose, camera_back},
    greeting_motion::{MotionPhase, MotionSample, Timeline},
    hair::{Hair, PARTICLES, breeze},
    hair_body::BodyBack,
    hair_ribbon,
    idle_wind::{IDLE_FRAMES, WIND_FRAMES},
    mesh::{self, Mesh},
    preview::{UI4_IMPORT_BUSY, gpu_error, ui_error, window_placement, write_geometry},
};
use floor_material::CLEAR_RGBA;
use foot_contact::FootProbe;
use hand_contact::HandProbe;
use reel_preview::FireflyPhase;
use sky_controls::SkyControls;
use sky_light::{Key, Levels, SkyLight};
use sky_palette::NormalPalette;
use top_controls::TopControls;

mod base_color;
mod belly_detail;
mod body_tint;
mod butterfly;
mod dance;
mod ear_detail;
mod eye_detail;
mod eye_sprite;
mod fire_light;
mod floor;
mod floor_material;
mod foot_contact;
mod hairband;
mod hand_contact;
mod heart_arm;
mod idle;
mod lico_graph;
mod live_butterfly;
mod normal_detail;
mod pelvis_detail;
mod pupil;
mod reel;
mod reel_draw;
mod reel_preview;
mod skin_study;
mod sky_controls;
mod sky_light;
mod sky_palette;
mod top_controls;

const SECOND_NS: u64 = 1_000_000_000;
const FRAME_BUDGET_NS: u64 = SECOND_NS / HZ as u64;
const REPORT_NS: u64 = 5 * SECOND_NS;
const POSES_PER_MESH: usize = 24;
const VERTEX_BYTES: usize = 32;
const WAIT_NS: u64 = 2 * SECOND_NS;
const DRAW_GROUPS: usize = 3;
const LIT_MATERIALS: usize = 2;
const MATERIALS: [&str; 4] = [
    mesh::FACE_MATERIAL,
    mesh::BODY_MATERIAL,
    mesh::HAIR_MATERIAL,
    "N00_000_00_FaceMouth_00_FACE (Instance)",
];
const NORMAL_MAP_SIZE: u32 = 1024;
const SKIN_NORMAL_MAPS: [&[u8]; LIT_MATERIALS] = [
    include_bytes!("../assets/AvatarSample_B-face-normal-1024.png"),
    include_bytes!("../assets/AvatarSample_B-body-normal-1024.png"),
];
const QUAD_VERTEX_COUNT: u32 = 4;
const HAIRBAND_COLUMN: usize = eye_sprite::SIZE + 1;
const _: () = assert!(HAIRBAND_COLUMN + hairband::WIDTH <= 512);
// Surface import and render completion can forward UI4's missing-window status.
const UI4_SURFACE_GONE: i32 = -3;
// USB HID keyboard-page usages.
const TOP_ROW_3: u16 = 0x20;
const SPACE: u16 = 0x2c;

const SHADER_BYTES: &[u8] = include_bytes!("../shaders/retained-unlit.bundle");
#[used]
#[unsafe(link_section = ".trueos.shaders")]
static RETAINED_SHADERS: [u8; SHADER_BYTES.len()] =
    *include_bytes!("../shaders/retained-unlit.bundle");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Appearance {
    Lighting,
    BaseColor,
    SkinStudy,
    Butterfly,
}

pub(super) async fn show(
    glb: &[u8],
    greeting_glb: Option<&[u8]>,
    motions: Option<&Path>,
    appearance: Appearance,
    msaa_samples: u8,
    eyes: EyeStyle,
) -> Result<()> {
    let controls = if appearance == Appearance::BaseColor {
        None
    } else {
        match SkyControls::open() {
            Ok(controls) => Some(controls),
            Err(error) => {
                report(format_args!(
                    "avatar lighting terminal unavailable: {error}"
                ));
                None
            }
        }
    };
    let levels = Levels::default();
    let (x, y, width, height) = window_placement()?;
    let mesh = mesh::prepare_materials(glb, width, height, &MATERIALS, true)?;
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
        .map_err(gpu_error)
        .inspect_err(|error| report(format_args!("open avatar device: {error:#}")))?;
    let shown = async {
        let reel = match motions {
            Some(path) => reel::Reel::load(path, glb, &MATERIALS, mesh.material_vertices[1].start)?,
            None => None,
        };
        let drawing_butterfly = appearance == Appearance::Butterfly;
        let studying_skin = matches!(appearance, Appearance::SkinStudy | Appearance::Butterfly);
        let motion = if drawing_butterfly {
            Some(reel_preview::Motion::butterflies(
                glb,
                mesh.material_vertices[1].end,
            )?)
        } else if let Some(reel) = reel {
            Some(reel_preview::Motion::Reel(reel))
        } else if studying_skin {
            Some(reel_preview::Motion::Nod(Animation::nod(
                glb, &MATERIALS, None,
            )?))
        } else {
            None
        };
        let color = appearance == Appearance::BaseColor;
        let (mut playback, framing) = if let Some(motion) = motion {
            let preview =
                reel_preview::Preview::new(device, &mesh, motion, glb, studying_skin, eyes).await?;
            let framing = preview.framing();
            (Playback::Reel(preview), framing)
        } else {
            let mut greeting = greeting_glb
                .map(|clip| Animation::prepare(clip, glb, &MATERIALS))
                .transpose()?;
            if let Some(clip) = &greeting {
                ensure!(clip.duration() <= 8.0, "unexpected Greeting duration");
            }
            let nod = Animation::nod(glb, &MATERIALS, greeting.as_mut())?;
            let mut sequence = AnimationSequence { greeting, nod };
            if !color {
                apply_normal_maps(glb, true, |section, normals| {
                    sequence.nod.apply_bind_normals(section, normals)?;
                    if let Some(greeting) = &mut sequence.greeting {
                        greeting.apply_bind_normals(section, normals)?;
                    }
                    Ok(())
                })
                .await?;
            }
            let renderer =
                AvatarRenderer::prepare(device, &mesh, &mut sequence, glb, color, eyes).await?;
            let skin = if color {
                Skin::BaseColor(base_color::retain(device).await?)
            } else {
                let view = crate::autobs::view_back(BASE_YAW);
                let light = SkyLight::new(view, levels);
                Skin::Lighting(NormalPalette::new(device, view, levels, &light).await?)
            };
            let framing = renderer.framing;
            (Playback::Retained(renderer, skin), framing)
        };
        let mut observer = Autobs::new((width, height), framing, mesh.pixels_per_unit);
        let mut frame = Frame::open_streaming(x, y, width, height).map_err(ui_error)?;
        let title = match appearance {
            Appearance::SkinStudy => "EmoModel · Skin",
            Appearance::Butterfly => "EmoModel · Butterfly",
            Appearance::BaseColor => "EmoModel · 4K Base Color",
            Appearance::Lighting => "EmoModel",
        };
        frame.set_title(title).map_err(ui_error)?;
        play(
            &mut frame,
            device,
            &mut playback,
            &mut observer,
            msaa_samples,
            title,
            controls,
        )
        .await
    }
    .await;
    let released = device.close().map_err(gpu_error);
    if let Err(error) = &shown {
        report(format_args!("avatar preview failed: {error:#}"));
    }
    shown?;
    released
}

async fn apply_normal_maps(
    glb: &[u8],
    full_maps: bool,
    mut apply: impl FnMut(usize, &[[f32; 3]]) -> Result<()>,
) -> Result<()> {
    let ear = ear_detail::EarDetail::load().await?;
    let local_body = if full_maps {
        None
    } else {
        Some((
            belly_detail::BellyDetail::load().await?,
            pelvis_detail::PelvisDetail::load().await?,
        ))
    };
    for (section, (material, png)) in MATERIALS.iter().zip(SKIN_NORMAL_MAPS).enumerate() {
        let image = if full_maps {
            let image = vmedia::decode(ImageFormat::Png, png)
                .await
                .map_err(gpu_error)
                .with_context(|| format!("decode the normal map for {material}"))?;
            ensure!(
                image.info.width == NORMAL_MAP_SIZE
                    && image.info.height == NORMAL_MAP_SIZE
                    && image.info.stride_bytes == NORMAL_MAP_SIZE * 4,
                "unexpected normal map extent for {material}"
            );
            Some(image)
        } else {
            None
        };
        let map = image
            .as_ref()
            .map(|image| (image.rgba.as_slice(), NORMAL_MAP_SIZE as usize));
        let mut normals = normal_detail::bake(glb, material, map, |uv| {
            if section == 0 {
                ear.tangent_offset(uv)
            } else {
                local_body
                    .as_ref()
                    .map_or(Vec2::ZERO, |(belly, pelvis)| {
                        belly.tangent_offset(uv) + pelvis.tangent_offset(uv)
                    })
                    + belly_detail::fold_offset(uv)
            }
        })?;
        if section == 1 && !full_maps {
            pelvis_detail::smooth_front_seam(&mut normals);
        }
        apply(section, &normals)?;
    }
    if full_maps {
        report(format_args!(
            "skin normal detail atlases={} extent={NORMAL_MAP_SIZE}x{NORMAL_MAP_SIZE} sampling=bind-vertex",
            SKIN_NORMAL_MAPS.len()
        ));
    } else {
        report(format_args!("belly and pelvis normal crops sampling=bind-vertex"));
    }
    report(format_args!(
        "shared mirrored ear relief sampling=bind-vertex"
    ));
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Motion(MotionPhase),
    Reel(&'static str),
    IdleWind,
    IdleSettle,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Motion(MotionPhase::Greeting) => "Greeting",
            Self::Motion(MotionPhase::ToNod) => "Greeting-to-Nod",
            Self::Motion(MotionPhase::Nod) => "Nod",
            Self::Motion(MotionPhase::FromNod) => "Nod-to-reverse-Greeting",
            Self::Motion(MotionPhase::ReverseGreeting) => "Reverse-Greeting",
            Self::Reel(name) => name,
            Self::IdleWind => "Idle-wind",
            Self::IdleSettle => "Idle-settle",
        }
    }
}

struct AnimationSequence {
    greeting: Option<Animation>,
    nod: Animation,
}

impl AnimationSequence {
    fn apply(&mut self, sample: MotionSample) -> &mut Animation {
        let clip = match sample.phase {
            MotionPhase::Greeting | MotionPhase::ReverseGreeting => self
                .greeting
                .as_mut()
                .expect("timeline has a Greeting clip"),
            _ => &mut self.nod,
        };
        clip.apply(sample.seconds);
        clip
    }
}

struct AvatarRenderer {
    device: Device,
    queue: Queue,
    atlas: Vec<RetainedMesh>,
    material_palette: RetainedTexture,
    draw_group_count: usize,
    poses: Vec<PoseDraw>,
    idle: Option<idle::Idle>,
    framing: Framing,
}

#[derive(Clone, Copy)]
struct PoseDraw {
    phase: Phase,
    mesh: usize,
    ranges: [RetainedDrawRange; DRAW_GROUPS],
}

#[derive(Clone, Copy)]
struct Staging {
    vertices: Buffer,
    indices: Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
}

fn append_range(
    indices: &mut Vec<u8>,
    draw: impl Iterator<Item = u32>,
) -> Result<RetainedDrawRange> {
    let first_index = u32::try_from(indices.len() / size_of::<u32>())?;
    indices.extend(draw.flat_map(u32::to_le_bytes));
    Ok(RetainedDrawRange {
        first_index,
        index_count: u32::try_from(indices.len() / size_of::<u32>())? - first_index,
    })
}

fn append_floor_quad(vertices: &mut Vec<u8>, positions: [[f32; 3]; 4], uv: [[f32; 2]; 4]) {
    for (position, uv) in positions.into_iter().zip(uv) {
        for value in position.into_iter().chain(Vec3::Y.to_array()).chain(uv) {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn quad_indices(base: u32, count: u32) -> impl Iterator<Item = u32> {
    (0..count).flat_map(move |quad| {
        [0, 1, 2, 2, 1, 3].map(|corner| base + quad * QUAD_VERTEX_COUNT + corner)
    })
}

fn section_indices<'a>(
    mesh: &'a Mesh,
    indices: &'a [u32],
    positions: &'a [[f32; 3]],
    sections: &'a [Range<usize>],
) -> impl Iterator<Item = u32> + 'a {
    sections
        .iter()
        .flat_map(|section| mesh.retained_triangles(indices, positions, section.clone()))
        .flatten()
}

fn advance_hair<'a>(
    animation: &'a mut Animation,
    hair: &mut Hair,
    body: &mut BodyBack,
    body_indices: &[u32],
    wind: Vec3,
) -> Result<&'a [[f32; 3]]> {
    let head = animation
        .head_from_bind()
        .context("missing hair root joint")?;
    let positions = animation.skin().positions;
    body.update(positions, body_indices);
    hair.step(head, wind, body);
    Ok(positions)
}

impl AvatarRenderer {
    async fn prepare(
        device: Device,
        mesh: &Mesh,
        sequence: &mut AnimationSequence,
        glb: &[u8],
        color: bool,
        eyes: EyeStyle,
    ) -> Result<Self> {
        let started = clock::monotonic_nanos();
        let queue = device.create_queue(QueueClass::Render).map_err(gpu_error)?;
        let eye = eye_detail::EyeDetail::load(glb).await?;
        let iris = eye_sprite::EyeSprite::load(eyes).await?;
        let hairband = hairband::Hairband::load(glb, mesh).await?;
        let greeting_duration = sequence.greeting.as_ref().map(Animation::duration);
        let nod_duration = sequence.nod.duration();
        let timeline = Timeline::new(greeting_duration, nod_duration);
        let pose_count = timeline.forward_frame_count();
        let frame_count = timeline.frame_count();
        let (source_vertices, compact_indices) = compact_mesh(mesh);
        let back = camera_back(BASE_YAW);
        sequence.nod.apply(sequence.nod.duration());
        let settled = sequence.nod.skin();
        let skin_indices = &mesh.indices[..mesh.skin_triangle_count * 3];
        let body_start = u32::try_from(mesh.material_vertices[1].start)?;
        let body_indices =
            &skin_indices[skin_indices.partition_point(|&index| index < body_start)..];
        let foot_probe = FootProbe::new(glb, mesh.material_vertices[1].start)?;
        let hand_probe = HandProbe::new(glb, mesh.material_vertices[1].start)?;
        let ground_y = skin_indices
            .iter()
            .map(|&index| settled.positions[index as usize][1])
            .fold(f32::INFINITY, f32::min)
            - 0.004;
        let strand_count = mesh.strand_guides.len();
        ensure!(strand_count > 0, "missing long back hair guides");
        let recovery = if greeting_duration.is_some() {
            ARM_RECOVERY_SECONDS
        } else {
            0.0
        };
        let nod_seconds = sequence.nod.duration() - recovery;
        let wind_at = |sample: MotionSample| match sample.phase {
            MotionPhase::Greeting | MotionPhase::ReverseGreeting => {
                breeze(sample.seconds / greeting_duration.unwrap_or_default())
            }
            MotionPhase::Nod => breeze((sample.seconds - recovery) / nod_seconds),
            MotionPhase::ToNod | MotionPhase::FromNod => Vec3::ZERO,
        };
        let initial = sequence.apply(timeline.sample(0));
        let head = initial
            .head_from_bind()
            .context("missing hair root joint")?;
        let mut hair = Hair::new(mesh.strand_guides.clone(), head);
        let mut body = BodyBack::default();
        body.update(initial.skin().positions, body_indices);
        // Settle before the held opening pose, then warm the complete motion loop.
        for _ in 0..2 * HZ {
            hair.step(head, Vec3::ZERO, &body);
        }
        let preroll = if greeting_duration.is_some() { 1 } else { 2 };
        let mut hair_frames = Vec::with_capacity(frame_count * strand_count);
        let mut loop_pose = Vec::with_capacity(strand_count);
        let mut contacts = floor::Atlas::new(pose_count);
        for cycle in 0..=preroll {
            for frame in 0..frame_count {
                let sample = timeline.sample(frame);
                let positions = advance_hair(
                    sequence.apply(sample),
                    &mut hair,
                    &mut body,
                    body_indices,
                    wind_at(sample),
                )?;
                if cycle == preroll {
                    if frame < pose_count {
                        let [[left_heel, left_toe], [right_heel, right_toe]] =
                            foot_probe.sample(positions).map(|foot| foot.points());
                        contacts.push(
                            ground_y,
                            [left_heel, left_toe, right_heel, right_toe],
                            hand_probe.sample(positions),
                        );
                    }
                    if let Some((reference, weight)) = sample.hair_reference() {
                        let start = reference * strand_count;
                        loop_pose.clear();
                        hair.append_loop_pose(
                            &hair_frames[start..start + strand_count],
                            weight,
                            &mut loop_pose,
                        );
                        hair_frames.extend_from_slice(&loop_pose);
                    } else {
                        hair_frames.extend(hair.strands().copied());
                    }
                }
            }
        }
        let eye_row = floor_material::SWATCH_ROWS;
        let iris_row = eye_row + eye_detail::HEIGHT;
        let texture_rows = iris_row + eye_sprite::SIZE.max(hairband::HEIGHT);
        let mut palette = contacts.bake_rows(&[512, 1024, 2048], 4096, texture_rows)?;
        for (column, row, width, pixels) in [
            (0, eye_row, eye_detail::WIDTH, eye.rgba()),
            (0, iris_row, eye_sprite::SIZE, iris.rgba()),
            (HAIRBAND_COLUMN, iris_row, hairband::WIDTH, hairband.rgba()),
        ] {
            for (i, pixel) in pixels.chunks_exact(4).enumerate() {
                palette.set(
                    column + i % width,
                    row + i / width,
                    [pixel[0], pixel[1], pixel[2]],
                );
            }
        }
        let [palette_width, palette_height] = palette.extent().map(|size| size as f32);
        let eye_uv = |[u, v]: [f32; 2]| {
            [
                u * eye_detail::WIDTH as f32 / palette_width,
                (eye_row as f32 + v * eye_detail::HEIGHT as f32) / palette_height,
            ]
        };
        let hairband_uv = |[u, v]: [f32; 2]| {
            [
                (HAIRBAND_COLUMN as f32 + u * hairband::WIDTH as f32) / palette_width,
                (iris_row as f32 + v * hairband::HEIGHT as f32) / palette_height,
            ]
        };
        let hair_uv = palette.swatch(0);
        let pupil_uv = palette.pupil();
        let strand_vertices = u32::try_from(PARTICLES * 2)?;
        let eye_base = u32::try_from(source_vertices.len())?;
        let pupil_base = eye_base + eye_sprite::VERTEX_COUNT;
        let detail_base = pupil_base + eye_sprite::PUPIL_VERTEX_COUNT;
        let hairband_base = detail_base + u32::try_from(eye.len())?;
        let first_strand_vertex = hairband_base + u32::try_from(hairband.len())?;
        // Keep the planar iris and authored details visible on both sides of
        // the retained camera arc. Depth still resolves the face and lids.
        let eye_details = eye
            .indices()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| triangle.map(|index| detail_base + index));
        let hairband_triangles = hairband
            .indices()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| triangle.map(|index| hairband_base + index));
        let detail_indices: Vec<_> = eye_sprite::EyeSprite::indices(eye_base)
            .chain(eye_sprite::EyeSprite::pupil_indices(pupil_base))
            .chain(eye_details)
            .chain(hairband_triangles)
            .flat_map(|[a, b, c]| [a, b, c, a, c, b])
            .collect();
        let strand_indices: Vec<_> = (0..u32::try_from(strand_count)?)
            .flat_map(|strand| {
                let first = first_strand_vertex + strand * strand_vertices;
                hair_ribbon::TRIANGLES
                    .into_iter()
                    .flatten()
                    .map(move |index| first + index)
            })
            .collect();
        let hair_vertex_count = u32::try_from(strand_count)? * strand_vertices;
        let variants = if greeting_duration.is_some() { 2 } else { 1 };
        let vertex_bytes = (first_strand_vertex as usize
            + hair_vertex_count as usize * variants
            + QUAD_VERTEX_COUNT as usize * floor::CONTACTS)
            * VERTEX_BYTES;
        let batch_poses = pose_count.min(POSES_PER_MESH);
        let vertex_capacity = vertex_bytes * batch_poses;
        let index_capacity = (compact_indices.len() * 2
            + (compact_indices.len() - skin_indices.len()) * 2 * (variants - 1)
            + (detail_indices.len() + strand_indices.len() + floor::CONTACTS * 6) * variants)
            * size_of::<u32>()
            * batch_poses;
        let vertex_buffer = device
            .create_buffer(
                vertex_capacity,
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(gpu_error)
            .context("allocate avatar vertex staging buffer")?;
        let index_buffer = device
            .create_buffer(
                index_capacity,
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(gpu_error)
            .context("allocate avatar index staging buffer")?;
        let staging = Staging {
            vertices: vertex_buffer,
            indices: index_buffer,
            vertex_capacity,
            index_capacity,
        };
        let mut vertices = Vec::with_capacity(vertex_capacity);
        let mut indices = Vec::with_capacity(index_capacity);
        let mut compact_positions = Vec::with_capacity(source_vertices.len());
        let mut atlas = Vec::with_capacity(pose_count.div_ceil(POSES_PER_MESH));
        let mut poses = vec![None; frame_count];
        let mut idle_base = None;
        let mut geometry_bytes = 0;
        let mut bounds = Bounds::new(false);
        let sections: [Vec<Range<usize>>; DRAW_GROUPS] = if color {
            let [face, body, hair, mouth] = mesh.material_triangles.as_slice() else {
                anyhow::bail!("unexpected avatar material sections");
            };
            [
                vec![face.clone(), mouth.clone()],
                vec![body.clone()],
                vec![hair.clone()],
            ]
        } else {
            [
                vec![0..mesh.skin_triangle_count],
                vec![mesh.skin_triangle_count..mesh.triangle_count],
                Vec::new(),
            ]
        };
        let draw_group_count = if color { 3 } else { 2 };
        let flat_group = draw_group_count - 1;
        let mut material_coordinates = vec![None; mesh.vertex_count()];
        for (slot, vertices) in mesh.material_vertices[LIT_MATERIALS..].iter().enumerate() {
            material_coordinates[vertices.clone()].fill(Some(palette.swatch(slot)));
        }
        if color {
            base_color::apply_coordinates(glb, mesh, &mut material_coordinates)?;
        }
        for start in (0..pose_count).step_by(POSES_PER_MESH) {
            let end = (start + POSES_PER_MESH).min(pose_count);
            let mesh_index = atlas.len();
            vertices.clear();
            indices.clear();
            for pose in start..end {
                let motion = timeline.sample(pose);
                let reverse = timeline.reverse_frame(pose);
                let sample = sequence.apply(motion).skin();
                ensure!(
                    sample.positions.len() == mesh.vertex_count(),
                    "avatar mesh vertex count changed"
                );
                compact_positions.clear();
                let body_vertex_offset = vertices.len();
                let mut append_vertex = |position: [f32; 3], normal, material_uv| {
                    bounds.include(Vec3::from_array(position));
                    for value in position.into_iter().chain(normal).chain(material_uv) {
                        vertices.extend_from_slice(&value.to_le_bytes());
                    }
                };
                for &source in &source_vertices {
                    let position = sample.positions[source];
                    let normal = sample.normals[source];
                    ensure!(
                        position.iter().all(|p| p.is_finite()),
                        "nonfinite avatar pose"
                    );
                    let material_uv = material_coordinates[source]
                        .unwrap_or_else(|| NormalPalette::coordinates(Vec3::from_array(normal)));
                    append_vertex(position, normal, material_uv);
                    compact_positions.push(position);
                }
                for (position, [u, v]) in iris.vertices(sample.positions) {
                    let uv = [u / palette_width, (iris_row as f32 + v) / palette_height];
                    append_vertex(position.to_array(), Vec3::NEG_Z.to_array(), uv);
                }
                let pupil_area =
                    pupil::retained_area(motion, nod_duration, greeting_duration.is_some());
                for position in iris.pupil_vertices(sample.positions, pupil_area) {
                    append_vertex(position.to_array(), Vec3::NEG_Z.to_array(), pupil_uv);
                }
                for (position, uv) in eye.vertices(sample.positions) {
                    append_vertex(position, Vec3::NEG_Z.to_array(), eye_uv(uv));
                }
                for (position, uv) in hairband.vertices(sample.positions) {
                    append_vertex(position, Vec3::NEG_Z.to_array(), hairband_uv(uv));
                }
                // Reverse playback shares the body pose. The hair was simulated in
                // playback order, so the same head pose can have different strands.
                for frame in [Some(pose), reverse].into_iter().flatten() {
                    let start = frame * strand_count;
                    for points in &hair_frames[start..start + strand_count] {
                        for vertex in hair_ribbon::vertices(points, back).into_iter().flatten() {
                            append_vertex(vertex.to_array(), back.to_array(), hair_uv);
                        }
                    }
                }
                let contact_offset = vertices.len();
                let contact_base = u32::try_from(contact_offset / VERTEX_BYTES)?;
                for (points, uv) in contacts.quads(pose, ground_y, &palette) {
                    append_floor_quad(&mut vertices, points, uv);
                }
                let vertex_end = u32::try_from(vertices.len() / VERTEX_BYTES)?;
                let contact_count = (vertex_end - contact_base) / QUAD_VERTEX_COUNT;
                let contact_indices = || quad_indices(contact_base, contact_count);
                let base_vertex = u32::try_from(body_vertex_offset / VERTEX_BYTES)?;
                let mut ranges = [RetainedDrawRange::default(); DRAW_GROUPS];
                for (group, (range, sections)) in ranges
                    .iter_mut()
                    .zip(&sections)
                    .take(draw_group_count)
                    .enumerate()
                {
                    let body_indices =
                        section_indices(mesh, &compact_indices, &compact_positions, sections);
                    let extra_indices = (group == flat_group)
                        .then_some(detail_indices.iter().chain(&strand_indices).copied())
                        .into_iter()
                        .flatten();
                    let draw = body_indices
                        .chain(extra_indices)
                        .map(|index| base_vertex + index)
                        .chain(
                            (group == flat_group)
                                .then(contact_indices)
                                .into_iter()
                                .flatten(),
                        );
                    *range = append_range(&mut indices, draw)?;
                }
                poses[pose] = Some(PoseDraw {
                    phase: Phase::Motion(motion.phase),
                    mesh: mesh_index,
                    ranges,
                });
                if let Some(frame) = reverse {
                    let mut reverse_ranges = ranges;
                    let body_indices = section_indices(
                        mesh,
                        &compact_indices,
                        &compact_positions,
                        &sections[flat_group],
                    );
                    let draw = body_indices
                        .chain(detail_indices.iter().copied())
                        .chain(
                            strand_indices
                                .iter()
                                .map(|&index| index + hair_vertex_count),
                        )
                        .map(|index| base_vertex + index)
                        .chain(contact_indices());
                    reverse_ranges[flat_group] = append_range(&mut indices, draw)?;
                    poses[frame] = Some(PoseDraw {
                        phase: Phase::Motion(timeline.sample(frame).phase),
                        mesh: mesh_index,
                        ranges: reverse_ranges,
                    });
                }
                if greeting_duration.is_none() && pose + 1 == pose_count {
                    // Idle frames share the terminal body pose and its contacts.
                    let body_end = body_vertex_offset + first_strand_vertex as usize * VERTEX_BYTES;
                    let mut shared_vertices = vertices[body_vertex_offset..body_end].to_vec();
                    shared_vertices.extend_from_slice(&vertices[contact_offset..]);
                    let mut shared_indices = Vec::new();
                    let mut shared_ranges = [RetainedDrawRange::default(); DRAW_GROUPS];
                    for (group, (range, sections)) in shared_ranges
                        .iter_mut()
                        .zip(&sections)
                        .take(draw_group_count)
                        .enumerate()
                    {
                        let body_indices =
                            section_indices(mesh, &compact_indices, &compact_positions, sections);
                        let details = (group == flat_group)
                            .then_some(detail_indices.iter().copied())
                            .into_iter()
                            .flatten();
                        let contact_quads = (group == flat_group)
                            .then(|| quad_indices(first_strand_vertex, contact_count))
                            .into_iter()
                            .flatten();
                        *range = append_range(
                            &mut shared_indices,
                            body_indices.chain(details).chain(contact_quads),
                        )?;
                    }
                    idle_base = Some(idle::IdleBase {
                        vertices: shared_vertices,
                        indices: shared_indices,
                        ranges: shared_ranges,
                    });
                }
            }
            write_geometry(device, vertex_buffer, &vertices)
                .context("upload avatar vertex staging buffer")?;
            write_geometry(device, index_buffer, &indices)
                .context("upload avatar index staging buffer")?;
            geometry_bytes += vertices.len() + indices.len();
            atlas.push(
                device
                    .create_retained_mesh(
                        vertex_buffer,
                        index_buffer,
                        RetainedMeshDescriptor {
                            vertex_count: u32::try_from(vertices.len() / VERTEX_BYTES)?,
                            index_count: u32::try_from(indices.len() / size_of::<u32>())?,
                            vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV,
                            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                            ..Default::default()
                        },
                    )
                    .map_err(gpu_error)
                    .with_context(|| format!("retain avatar poses {start} through {}", end - 1))?,
            );
        }
        let poses = poses
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .context("incomplete motion loop geometry")?;
        drop((vertices, indices));
        let idle = if let Some(base) = idle_base {
            let idle = idle::Idle::bake(
                device,
                staging,
                base,
                idle::IdleHair {
                    hair: &mut hair,
                    head: sequence
                        .nod
                        .head_from_bind()
                        .context("missing idle hair root joint")?,
                    body: &body,
                    view_back: back,
                    hair_uv,
                },
                |position| bounds.include(position),
            )?;
            geometry_bytes += idle.geometry_bytes();
            Some(idle)
        } else {
            None
        };
        let palette_png = palette.png().context("encode the contact material atlas")?;
        let material_palette = vmedia::decode_retained(device, ImageFormat::Png, &palette_png)
            .await
            .map_err(gpu_error)
            .context("retain the flat materials and ground contact masks")?;
        let elapsed_ms = (clock::monotonic_nanos() - started) / 1_000_000;
        let max_indices = poses
            .iter()
            .map(|pose| {
                pose.ranges
                    .iter()
                    .map(|range| range.index_count)
                    .sum::<u32>()
            })
            .max()
            .unwrap_or(0);
        report(format_args!(
            "avatar ready target_hz={HZ} clips={} baked_s={:.3} poses={frame_count} body_poses={pose_count} idle_wind_frames={} idle_settle_frames={} resident_meshes={} geometry_bytes={geometry_bytes} triangles={} hair_triangles={} hair_strands={strand_count} segments_per_strand={} max_indices={max_indices} cull=back bake_upload_ms={elapsed_ms}",
            if greeting_duration.is_some() {
                "Greeting,Nod,Reverse-Greeting"
            } else {
                "Nod"
            },
            frame_count as f32 / HZ as f32,
            if idle.is_some() { WIND_FRAMES } else { 0 },
            if idle.is_some() {
                IDLE_FRAMES - WIND_FRAMES
            } else {
                0
            },
            atlas.len() + idle.as_ref().map_or(0, idle::Idle::mesh_count),
            mesh.triangle_count + strand_indices.len() / 3,
            mesh.hair_triangle_count + strand_indices.len() / 3,
            PARTICLES - 1,
        ));
        Ok(Self {
            device,
            queue,
            atlas,
            material_palette,
            draw_group_count,
            poses,
            idle,
            framing: bounds.framing(0.0),
        })
    }

    fn pose(&self, elapsed_ns: u64) -> (usize, Phase) {
        let tick = elapsed_ns * u64::from(HZ) / SECOND_NS;
        let period = self.poses.len() + if self.idle.is_some() { IDLE_FRAMES } else { 0 };
        let pose = (tick % period as u64) as usize;
        let phase = if let Some(motion) = self.poses.get(pose) {
            motion.phase
        } else if pose - self.poses.len() < WIND_FRAMES {
            Phase::IdleWind
        } else {
            Phase::IdleSettle
        };
        (pose, phase)
    }

    fn submit(
        &self,
        surface: Ui4Surface,
        camera: RetainedCamera,
        pose: usize,
        skin: &Skin,
    ) -> Result<(), i32> {
        let mut submit = RetainedTexturedFrameV1 {
            frame: RetainedFrameSubmit {
                clear_rgba8_srgb: CLEAR_RGBA,
                camera,
                ..Default::default()
            },
            ..Default::default()
        };
        let base_groups = self.draw_group_count;
        let (mesh, groups) = if let Some(motion) = self.poses.get(pose) {
            submit.ranges[..base_groups].copy_from_slice(&motion.ranges[..base_groups]);
            (self.atlas[motion.mesh], base_groups)
        } else {
            let (mesh, shared, hair) = self
                .idle
                .as_ref()
                .expect("idle playback has a mesh")
                .draw(pose - self.poses.len());
            submit.ranges[..base_groups].copy_from_slice(&shared[..base_groups]);
            submit.ranges[base_groups] = hair;
            (mesh, base_groups + 1)
        };
        submit.frame.seed_count = groups as u32;
        submit.frame.seeds[..groups].fill(RetainedTransformSeed {
            scale: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        submit.textures[..groups].fill(self.material_palette.id().raw());
        match skin {
            Skin::Lighting(light) => submit.textures[0] = light.texture().id().raw(),
            Skin::BaseColor([face, body]) => {
                submit.textures[0] = face.id().raw();
                submit.textures[1] = body.id().raw();
            }
        }
        self.device
            .submit_retained_textured_frame_v1(self.queue, surface, mesh, submit)
            .map(|_| ())
    }
}

enum Playback<'a> {
    Retained(AvatarRenderer, Skin),
    Reel(reel_preview::Preview<'a>),
}

fn import_surface(device: Device, window: u32, deadline: u64) -> Result<Ui4Surface, i32> {
    loop {
        match device.acquire_ui4_surface(window) {
            Ok(surface) => return Ok(surface),
            Err(vgpu::ERR_BUSY | UI4_IMPORT_BUSY) if clock::monotonic_nanos() < deadline => {
                vsys::sleep_ms(1);
            }
            Err(code) => return Err(code),
        }
    }
}

enum Skin {
    Lighting(NormalPalette),
    BaseColor([RetainedTexture; 2]),
}

fn camera_view_back(camera: RetainedCamera) -> Vec3 {
    let [x, y, z, _] = camera.forward_far;
    -Vec3::new(x, y, z)
}

fn compact_mesh(mesh: &Mesh) -> (Vec<usize>, Vec<u32>) {
    let mut mapping = vec![u32::MAX; mesh.vertex_count()];
    let mut source_vertices = Vec::new();
    let indices = mesh
        .indices
        .iter()
        .map(|&index| {
            let source = index as usize;
            if mapping[source] == u32::MAX {
                mapping[source] = source_vertices.len() as u32;
                source_vertices.push(source);
            }
            mapping[source]
        })
        .collect();
    (source_vertices, indices)
}

struct ResizeTrace {
    start: u64,
    duration: u64,
    next_sample: u8,
}

struct ObserverSample {
    resize: u64,
    elapsed: u64,
    extent: (u32, u32),
    pose: ObserverPose,
}

impl ResizeTrace {
    fn sample(
        &mut self,
        time: u64,
        extent: (u32, u32),
        pose: ObserverPose,
    ) -> Option<ObserverSample> {
        let elapsed = time.saturating_sub(self.start);
        let step = (elapsed.saturating_mul(3) / self.duration.max(1)).min(3) as u8;
        if step < self.next_sample {
            return None;
        }
        self.next_sample = step + 1;
        Some(ObserverSample {
            resize: self.start,
            elapsed,
            extent,
            pose,
        })
    }
}

impl ObserverSample {
    fn report(&self, serial: u64) {
        report(format_args!(
            "autobs presented resize={} extent={}x{} serial={serial} elapsed_ms={} zoom={:.3} distance={:.3} yaw_deg={:.2}",
            self.resize,
            self.extent.0,
            self.extent.1,
            self.elapsed / 1_000_000,
            self.pose.zoom,
            self.pose.distance,
            self.pose.yaw_offset.to_degrees(),
        ));
    }
}

async fn play(
    frame: &mut Frame,
    device: Device,
    playback: &mut Playback<'_>,
    observer: &mut Autobs,
    msaa_samples: u8,
    title: &str,
    mut controls: Option<SkyControls>,
) -> Result<()> {
    let mut pending_resize = None;
    let mut resizing = false;
    let mut epoch = None;
    let mut next_frame = 0;
    let mut stats = Timing::default();
    let mut receipts = VecDeque::new();
    let mut last_camera_pose = observer.pose(clock::monotonic_nanos());
    let mut resize_trace = None;
    let mut previous_phase = None;
    let mut display_backdrop = None::<(Backdrop, [u8; 3])>;
    let mut levels = Levels::default();
    let mut presented_levels = None;
    let lighting_active = !matches!(playback, Playback::Retained(_, Skin::BaseColor(_)));
    let (msaa2_available, msaa4_available) = if matches!(playback, Playback::Reel(_)) {
        device.info().map_or((false, false), |info| {
            (info.supports_indexed_msaa2(), info.supports_indexed_msaa4())
        })
    } else {
        (false, false)
    };
    let mut tops = if matches!(playback, Playback::Reel(_)) {
        Some(TopControls::new(frame).map_err(ui_error)?)
    } else {
        None
    };
    let mut requested_samples = match msaa_samples {
        2 if msaa2_available => 2,
        4 if msaa4_available => 4,
        _ => 1,
    };
    let mut previous_samples = 1;
    let mut previous_top = None;
    let mut butterfly_requested = false;
    if msaa2_available {
        report(format_args!("avatar 2x MSAA available, press M to toggle"));
    } else if msaa4_available {
        report(format_args!(
            "avatar MSAA available, press M to switch between 1x and 4x"
        ));
    }
    loop {
        if let Some(terminal) = &mut controls {
            match terminal.poll() {
                Ok(Some(selected)) => levels = selected,
                Ok(None) => (),
                Err(error) => {
                    report(format_args!("avatar lighting terminal closed: {error}"));
                    controls = None;
                }
            }
        }
        if matches!(playback, Playback::Reel(_)) {
            // Let the shared graph observer and its asynchronous file requests run.
            tokio::task::yield_now().await;
        }
        if let Some(tops) = &mut tops {
            match tops.tick(frame) {
                Ok(()) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
            loop {
                match frame.take_physical_keyboard_event() {
                    Ok(Some(event))
                        if event.kind == input::KEYBOARD_OUTPUT_KIND_PHYSICAL
                            && event.modifiers == 0
                            && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0 =>
                    {
                        match event.key_code {
                            TOP_ROW_3 => butterfly_requested = true,
                            SPACE => tops.toggle_random(),
                            _ => (),
                        }
                    }
                    Ok(Some(_)) => (),
                    Ok(None) | Err(Error::Busy) => break,
                    Err(Error::NotFound) => return Ok(()),
                    Err(error) => return Err(ui_error(error)),
                }
            }
            loop {
                match frame.take_keyboard_event() {
                    Ok(Some(event)) if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT => {
                        let Some(key) = char::from_u32(event.codepoint) else {
                            continue;
                        };
                        let selected = match key {
                            'p' | 'P' => {
                                if let Playback::Reel(preview) = &mut *playback {
                                    preview.toggle_pelvis_markers();
                                }
                                continue;
                            }
                            'M' if msaa4_available => 4,
                            'm' | 'M' if msaa2_available => 2,
                            'm' if msaa4_available => 4,
                            _ => {
                                tops.key(key);
                                continue;
                            }
                        };
                        requested_samples = if requested_samples == selected {
                            1
                        } else {
                            selected
                        };
                    }
                    Ok(Some(_)) => (),
                    Ok(None) => break,
                    Err(Error::NotFound) => return Ok(()),
                    Err(error) => return Err(ui_error(error)),
                }
            }
        }
        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => pending_resize = Some((event.width, event.height)),
                Ok(None) => break,
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }
        if let Some(extent) = pending_resize {
            match frame.resize(extent.0, extent.1) {
                Ok(()) => {
                    pending_resize = None;
                    stats = Timing::default();
                    receipts.clear();
                    resize_trace = None;
                    resizing = true;
                }
                Err(Error::Busy) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }
        let wait_start = clock::monotonic_nanos();
        if !resizing && wait_start < next_frame {
            vsys::sleep_ms((next_frame - wait_start).div_ceil(1_000_000));
            continue;
        }
        let extent = (frame.width(), frame.height());
        if let Some((backdrop, applied)) = &mut display_backdrop {
            let (rgb, settled) = backdrop.sample_entry_at(wait_start);
            let result = if rgb == *applied {
                Ok(())
            } else {
                frame.set_display_bottom_color(rgb)
            };
            match result {
                Ok(()) => {
                    *applied = rgb;
                    if settled {
                        display_backdrop = None;
                    }
                }
                Err(Error::Busy) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => {
                    report(format_args!(
                        "avatar display bottom color failed: {error:?}"
                    ));
                    display_backdrop = None;
                }
            }
        }
        let samples = if vgpu::indexed_msaa_extent_supported(extent.0, extent.1) {
            requested_samples
        } else {
            1
        };
        if let Some(tops) = &tops
            && (samples != previous_samples || Some(tops.style) != previous_top)
        {
            match frame.set_title(&tops.title(title, samples)) {
                Ok(()) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
            if let Playback::Reel(preview) = &mut *playback {
                preview.set_top(tops.style)?;
            }
            previous_top = Some(tops.style);
        }
        if samples != previous_samples {
            stats = Timing::default();
            receipts.clear();
            report(format_args!(
                "avatar sampling requested={requested_samples} effective={samples} extent={}x{}",
                extent.0, extent.1,
            ));
            previous_samples = samples;
        }
        let deadline = wait_start + WAIT_NS;
        loop {
            match frame.begin_gpu_frame() {
                Ok(()) => break,
                Err(Error::Busy) if clock::monotonic_nanos() < deadline => vsys::sleep_ms(1),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }
        let admitted = clock::monotonic_nanos();
        let epoch = *epoch.get_or_insert(admitted);
        let elapsed = admitted - epoch;
        // Anchor deadlines to elapsed time so millisecond wake-ups cannot accumulate drift.
        let tick = elapsed * u64::from(HZ) / SECOND_NS;
        next_frame = epoch + ((tick + 1) * SECOND_NS).div_ceil(u64::from(HZ));
        let (pose, phase, framing_changed) = match playback {
            Playback::Retained(renderer, _) => {
                let (pose, phase) = renderer.pose(elapsed);
                (pose, phase, false)
            }
            Playback::Reel(preview) => {
                let framing =
                    preview.sync_motion(elapsed, std::mem::take(&mut butterfly_requested));
                let framing_changed = observer.set_framing(framing, admitted);
                let (pose, phase) = preview.pose(elapsed, admitted);
                (pose, Phase::Reel(phase), framing_changed)
            }
        };
        // Keep the last published camera through the atomic backing swap.
        let camera_pose = if resizing && !framing_changed {
            last_camera_pose
        } else {
            observer.pose(admitted)
        };
        let camera = observer.camera(camera_pose, extent);
        let view_back = camera_view_back(camera);
        let sky = SkyLight::new(view_back, levels);
        if let Playback::Retained(_, Skin::Lighting(light)) = &mut *playback {
            light.update(device, view_back, levels, &sky).await?;
        }
        let surface = match import_surface(device, frame.window_id(), admitted + WAIT_NS) {
            Ok(surface) => surface,
            Err(UI4_SURFACE_GONE) => return Ok(()),
            Err(code) => return Err(gpu_error(code)),
        };
        let submit_started = clock::monotonic_nanos();
        let rendered = match playback {
            Playback::Retained(renderer, skin) => renderer.submit(surface, camera, pose, skin),
            Playback::Reel(preview) => {
                preview.submit(surface, camera, pose, elapsed, admitted, samples, &sky)
            }
        };
        match rendered {
            Ok(()) => (),
            Err(vgpu::ERR_BUSY) => {
                // The consumed import cancels the write lease on a busy submit.
                stats.busy += 1;
                continue;
            }
            Err(vgpu::ERR_UNSUPPORTED) if samples > 1 => {
                requested_samples = 1;
                report(format_args!(
                    "avatar MSAA admission declined ({samples}x), resuming at 1x"
                ));
                continue;
            }
            Err(UI4_SURFACE_GONE) => return Ok(()),
            Err(code) => return Err(gpu_error(code)).context("draw the avatar pose"),
        }
        let submitted = clock::monotonic_nanos();
        let firefly = match playback {
            Playback::Reel(preview) => preview.firefly_phase(),
            Playback::Retained(..) => FireflyPhase::Idle,
        };
        let damage = Damage::full(extent.0, extent.1);
        let receipt = loop {
            match frame.publish_tracked(damage) {
                Ok(serial) => break Some(serial),
                Err(Error::Busy) if clock::monotonic_nanos() < submitted + WAIT_NS => {
                    vsys::sleep_ms(1)
                }
                Err(Error::NotFound) => return Ok(()),
                Err(Error::InvalidState) if resizing => break None,
                Err(error) => return Err(ui_error(error)),
            }
        };
        let finished = clock::monotonic_nanos();
        if let Some(serial) = receipt {
            if resizing {
                observer.retarget(extent, camera_pose, finished);
                resize_trace = Some(ResizeTrace {
                    start: finished,
                    duration: observer.duration_ns(),
                    next_sample: 0,
                });
            }
            last_camera_pose = camera_pose;
            if let Playback::Reel(preview) = &mut *playback
                && preview.on_published(pose, finished, camera, extent)
            {
                match frame.display_bottom_color() {
                    Ok(rgb) => display_backdrop = Some((Backdrop::new(rgb, finished), rgb)),
                    Err(Error::NotFound) => return Ok(()),
                    Err(error) => report(format_args!(
                        "avatar read display bottom color failed: {error:?}"
                    )),
                }
            }
            stats.add(wait_start, admitted, finished, submitted - submit_started);
            let camera_sample = resize_trace
                .as_mut()
                .and_then(|trace| trace.sample(admitted, extent, camera_pose));
            receipts.push_back((serial, camera_sample, phase, pose, levels, firefly));
        }
        resizing = false;
        // UI4 retains eight recent display receipts. Leave delayed ones in the queue
        // without stalling the producer when the compositor is busy or obscured.
        let mut remaining = receipts.len();
        while remaining > 0 {
            let receipt = receipts.pop_front().expect("pending presentation receipt");
            let (serial, camera_sample, phase, pose, levels, firefly) = receipt;
            match frame.was_presented(serial) {
                Ok(true) => {
                    stats.displayed += 1;
                    if previous_phase != Some(phase) {
                        previous_phase = Some(phase);
                        report(format_args!(
                            "avatar playback phase={} pose={pose} present_serial={serial}",
                            phase.name()
                        ));
                    }
                    if lighting_active && presented_levels != Some((levels, firefly)) {
                        presented_levels = Some((levels, firefly));
                        let key = match levels.key {
                            Key::Neutral => "white".into(),
                            Key::Sunrise => "sunrise".into(),
                            Key::Kelvin(temperature) => format!("{temperature}K"),
                        };
                        report(format_args!(
                            "avatar light levels sky={} sheen={} key={key} firefly={} effect={} present_serial={serial} phase={}",
                            levels.sky,
                            levels.sheen,
                            if levels.firefly { "on" } else { "off" },
                            firefly.name(),
                            phase.name(),
                        ));
                    }
                    if let Some(sample) = camera_sample {
                        sample.report(serial);
                    }
                }
                Ok(false) => {
                    receipts.push_back((serial, camera_sample, phase, pose, levels, firefly));
                }
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
            remaining -= 1;
        }
        while receipts.len() > 8 {
            receipts.pop_front();
        }
        stats.report_if_ready(finished, extent, samples);
    }
}

#[derive(Default)]
struct Timing {
    started: Option<u64>,
    last_finish: Option<u64>,
    active_ns: Vec<u64>,
    submit_ns: Vec<u64>,
    acquire_ns: Vec<u64>,
    gap_ns: Vec<u64>,
    displayed: u32,
    busy: u32,
}

impl Timing {
    fn add(&mut self, acquire_start: u64, admitted: u64, finished: u64, submitted_ns: u64) {
        self.started.get_or_insert(admitted);
        if let Some(previous) = self.last_finish.replace(finished) {
            self.gap_ns.push(finished - previous);
        }
        self.active_ns.push(finished - admitted);
        self.submit_ns.push(submitted_ns);
        self.acquire_ns.push(admitted - acquire_start);
    }

    fn report_if_ready(&mut self, now: u64, (width, height): (u32, u32), samples: u8) {
        let Some(started) = self.started else { return };
        let elapsed = now - started;
        if elapsed < REPORT_NS {
            return;
        }
        let count = self.active_ns.len();
        let over = self
            .active_ns
            .iter()
            .filter(|&&ns| ns > FRAME_BUDGET_NS)
            .count();
        let seconds = elapsed as f64 / SECOND_NS as f64;
        let (active_avg, active_p95, active_max) = distribution_ms(&mut self.active_ns);
        let (submit_avg, submit_p95, submit_max) = distribution_ms(&mut self.submit_ns);
        let (_, acquire_p95, acquire_max) = distribution_ms(&mut self.acquire_ns);
        let (_, gap_p95, gap_max) = distribution_ms(&mut self.gap_ns);
        report(format_args!(
            "avatar timing extent={width}x{height} target_hz={HZ} published={count} presented={} window_s={seconds:.3} publish_hz={:.2} present_hz={:.2} active_ms_avg={active_avg:.3} active_ms_p95={active_p95:.3} active_ms_max={active_max:.3} submit_ms_avg={submit_avg:.3} submit_ms_p95={submit_p95:.3} submit_ms_max={submit_max:.3} back_buffer_ms_p95={acquire_p95:.3} back_buffer_ms_max={acquire_max:.3} frame_gap_ms_p95={gap_p95:.3} frame_gap_ms_max={gap_max:.3} over_budget={over} submit_busy={} samples={samples}",
            self.displayed,
            count as f64 / seconds,
            f64::from(self.displayed) / seconds,
            self.busy,
        ));
        self.started = Some(now);
        self.active_ns.clear();
        self.submit_ns.clear();
        self.acquire_ns.clear();
        self.gap_ns.clear();
        self.displayed = 0;
        self.busy = 0;
    }
}

fn distribution_ms(samples: &mut [u64]) -> (f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    samples.sort_unstable();
    let sum: u64 = samples.iter().sum();
    let percentile = (samples.len() * 95).div_ceil(100) - 1;
    (
        sum as f64 / samples.len() as f64 / 1_000_000.0,
        samples[percentile] as f64 / 1_000_000.0,
        samples[samples.len() - 1] as f64 / 1_000_000.0,
    )
}

fn report(message: std::fmt::Arguments<'_>) {
    let _ = logl::log_record(logl::level::IMPORTANT, "emomodel", message);
}
