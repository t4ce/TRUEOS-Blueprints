//! First executable Helio/SceneDB territory on the VMX WGPU adapter.
//!
//! This uses Helio's canonical `SceneObject` component and SceneDB mirror
//! machinery. It is not yet the high-level `helio::Scene`, whose constructor
//! correctly requires the texture/sampler part of WGPU that VMX still lacks.

use std::sync::Arc;

use helio_scenedb::{
    DirtyTrackedReallocationPolicy, SceneAuthority, SceneAuthorityConfig,
    SceneAuthoritySubsystemConfig, SceneObject, SceneObjectRenderRow, SceneObjectSpatialRow,
    register_scene_component_buffers,
};

pub struct SceneDbProbe {
    pub chunk_objects: u32,
    pub row_span: u32,
    pub reused_row: u32,
    pub flush_ranges: u32,
    pub flush_bytes: u64,
    pub stale_after_despawn: bool,
}

pub fn probe_partner_lifecycle(
    chunks: &[crate::voxel::WorldChunk],
) -> Result<SceneDbProbe, &'static str> {
    if chunks.is_empty() {
        return Err("world has no chunks to publish");
    }
    let (device, queue) = crate::wgpu_vmx::open_device_queue().map_err(|_| "open WGPU VMX")?;
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let mut config = SceneAuthorityConfig::default();
    config.initial_entity_capacity = 4;
    config.dirty_tracked_reallocation = DirtyTrackedReallocationPolicy::RewriteFromCpuShadow;
    config.subsystems = SceneAuthoritySubsystemConfig::SPRITE_STANDALONE;
    let mut authority = SceneAuthority::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        config,
        |store, device| register_scene_component_buffers(store, 4, device),
    );

    let entities: Vec<_> = chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| authority.insert(object_row(chunk, index as u32)))
        .collect();
    let entity = entities[chunks.len() / 2];
    let initial_row = authority
        .gpu_row::<SceneObject>(entity)
        .ok_or("insert did not allocate a component row")?;
    let mut flush_ranges = 0;
    let mut flush_bytes = 0;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    let chunk_objects = authority.gpu_live_count::<SceneObject>();
    let row_span = authority.gpu_row_span::<SceneObject>();
    if chunk_objects != chunks.len() as u32 || row_span < chunk_objects {
        return Err("world chunks did not occupy component-local rows");
    }

    authority
        .edit_gpu::<SceneObject, _>(entity, |object| {
            object.spatial.model[13] += 1.0;
            object.spatial.sphere[1] += 1.0;
        })
        .ok_or("edit lost the canonical object")?;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    let edited_y = chunks[chunks.len() / 2].center[1] + 1.0;
    if authority.get::<SceneObject>(entity).unwrap().spatial.model[13] != edited_y {
        return Err("edited transform was not canonical");
    }

    authority
        .remove::<SceneObject>(entity)
        .ok_or("remove lost the canonical object")?;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    if authority.gpu_row::<SceneObject>(entity).is_some() {
        return Err("removed object retained a GPU partner row");
    }

    if !authority.replace_gpu(
        entity,
        object_row(&chunks[chunks.len() / 2], chunks.len() as u32),
    ) {
        return Err("reinsert through mirror-aware replacement failed");
    }
    let reused_row = authority
        .gpu_row::<SceneObject>(entity)
        .ok_or("reinsert did not allocate a component row")?;
    if reused_row != initial_row {
        return Err("single-row free list did not reuse its stable row");
    }
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;

    for entity in entities {
        if !authority.despawn(entity) {
            return Err("despawn rejected a live world chunk entity");
        }
    }
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    let stale_after_despawn = !authority.is_alive(entity)
        && authority.get::<SceneObject>(entity).is_none()
        && authority.gpu_row::<SceneObject>(entity).is_none();
    if !stale_after_despawn {
        return Err("despawn did not invalidate entity and partner row");
    }

    Ok(SceneDbProbe {
        chunk_objects,
        row_span,
        reused_row,
        flush_ranges,
        flush_bytes,
        stale_after_despawn,
    })
}

fn flush_checked(
    authority: &SceneAuthority,
    device: &wgpu::Device,
    ranges: &mut u32,
    bytes: &mut u64,
) -> Result<(), &'static str> {
    let stats = authority.flush_gpu();
    *ranges = ranges.saturating_add(stats.ranges);
    *bytes = bytes.saturating_add(stats.bytes);
    if crate::wgpu_vmx::take_device_error(device).is_some() {
        return Err("VMX queue rejected a SceneDB mirror upload");
    }
    Ok(())
}

fn object_row(chunk: &crate::voxel::WorldChunk, mesh_row: u32) -> SceneObject {
    let mut model = [0.0; 16];
    model[0] = 1.0;
    model[5] = 1.0;
    model[10] = 1.0;
    model[15] = 1.0;
    model[12] = chunk.center[0];
    model[13] = chunk.center[1];
    model[14] = chunk.center[2];
    SceneObject {
        mesh_handle_bits: 0,
        material_handle_bits: 0,
        groups: 0,
        user_tag: 0x4845_4c49_4f56 ^ u64::from(mesh_row),
        spatial: SceneObjectSpatialRow {
            model,
            normal_mat: [0.0; 12],
            sphere: [
                chunk.center[0],
                chunk.center[1],
                chunk.center[2],
                chunk.radius,
            ],
            flags: 0,
            _pad: [0; 3],
        },
        render: SceneObjectRenderRow {
            mesh_row,
            material_row: 0,
            lightmap_index: u32::MAX,
            reserved: 0,
        },
        movability: 1,
        _pad: 0,
    }
}
