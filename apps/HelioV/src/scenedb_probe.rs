//! First executable Helio/SceneDB territory on the VMX WGPU adapter.
//!
//! This uses Helio's canonical `SceneObject` component and SceneDB mirror
//! machinery. It is not yet the high-level `helio::Scene`, whose constructor
//! correctly requires the texture/sampler part of WGPU that VMX still lacks.

use std::sync::Arc;

use helio_scenedb::{
    SceneAuthority, SceneAuthorityConfig, SceneAuthoritySubsystemConfig, SceneObject,
    SceneObjectRenderRow, SceneObjectSpatialRow, register_scene_component_buffers,
};

pub struct SceneDbProbe {
    pub reused_row: u32,
    pub flush_ranges: u32,
    pub flush_bytes: u64,
    pub stale_after_despawn: bool,
}

pub fn probe_partner_lifecycle() -> Result<SceneDbProbe, &'static str> {
    let (device, queue) = crate::wgpu_vmx::open_device_queue().map_err(|_| "open WGPU VMX")?;
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let mut config = SceneAuthorityConfig::default();
    config.initial_entity_capacity = 4;
    config.subsystems = SceneAuthoritySubsystemConfig::SPRITE_STANDALONE;
    let mut authority = SceneAuthority::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        config,
        |store, device| register_scene_component_buffers(store, 4, device),
    );

    let original = object_row(0.0);
    let entity = authority.insert(original);
    let initial_row = authority
        .gpu_row::<SceneObject>(entity)
        .ok_or("insert did not allocate a component row")?;
    let mut flush_ranges = 0;
    let mut flush_bytes = 0;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;

    authority
        .edit_gpu::<SceneObject, _>(entity, |object| {
            object.spatial.model[12] = 3.0;
            object.spatial.sphere[0] = 3.5;
        })
        .ok_or("edit lost the canonical object")?;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    if authority.get::<SceneObject>(entity).unwrap().spatial.model[12] != 3.0 {
        return Err("edited transform was not canonical");
    }

    authority
        .remove::<SceneObject>(entity)
        .ok_or("remove lost the canonical object")?;
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    if authority.gpu_row::<SceneObject>(entity).is_some() {
        return Err("removed object retained a GPU partner row");
    }

    if !authority.replace_gpu(entity, object_row(7.0)) {
        return Err("reinsert through mirror-aware replacement failed");
    }
    let reused_row = authority
        .gpu_row::<SceneObject>(entity)
        .ok_or("reinsert did not allocate a component row")?;
    if reused_row != initial_row {
        return Err("single-row free list did not reuse its stable row");
    }
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;

    if !authority.despawn(entity) {
        return Err("despawn rejected the live entity");
    }
    flush_checked(&authority, &device, &mut flush_ranges, &mut flush_bytes)?;
    let stale_after_despawn = !authority.is_alive(entity)
        && authority.get::<SceneObject>(entity).is_none()
        && authority.gpu_row::<SceneObject>(entity).is_none();
    if !stale_after_despawn {
        return Err("despawn did not invalidate entity and partner row");
    }

    Ok(SceneDbProbe {
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

fn object_row(translation_x: f32) -> SceneObject {
    let mut model = [0.0; 16];
    model[0] = 1.0;
    model[5] = 1.0;
    model[10] = 1.0;
    model[15] = 1.0;
    model[12] = translation_x;
    SceneObject {
        mesh_handle_bits: 0,
        material_handle_bits: 0,
        groups: 0,
        user_tag: 0x4845_4c49_4f56,
        spatial: SceneObjectSpatialRow {
            model,
            normal_mat: [0.0; 12],
            sphere: [translation_x + 0.5, 0.5, 0.5, 1.0],
            flags: 0,
            _pad: [0; 3],
        },
        render: SceneObjectRenderRow {
            mesh_row: 0,
            material_row: 0,
            lightmap_index: u32::MAX,
            reserved: 0,
        },
        movability: 1,
        _pad: 0,
    }
}
