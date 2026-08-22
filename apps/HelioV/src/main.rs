//! HelioV: the native Helio + SceneDB Blueprint bring-up application.
//!
//! This is intentionally not a port of Stratum's deprecated `voxel_world`
//! runtime. The voxel world is only the product target. Geometry is authored
//! directly in Helio types and, once the custom WGPU backend is available,
//! scene lifetime and rendering remain owned by Helio and SceneDB.

mod backend_contract;
mod platform;
mod scenedb_probe;
mod ui4_input;
mod voxel;
mod wgpu_vmx;

use trueos::{logl, vsys};

fn main() {
    let world = voxel::build_voxel_world();
    let fingerprint = voxel::mesh_fingerprint(&world.mesh);

    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioV: real Helio Blueprint entered; world={}x{} chunks={} voxels={} water={} landmarks={} vertices={} indices={} mesh_fingerprint=0x{fingerprint:016x}",
            voxel::WORLD_SIDE,
            voxel::WORLD_SIDE,
            world.chunks.len(),
            world.solid_voxels,
            world.water_voxels,
            world.landmark_voxels,
            world.mesh.vertices.len(),
            world.mesh.indices.len(),
        ),
    );
    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioV: adapter contract={} interface={}",
            backend_contract::REVISION,
            backend_contract::custom_device_interface(),
        ),
    );
    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioV: scene baseline stage={} policy=authenticated-WGPU-package/no-texture-assets/no-alternate-renderer",
            wgpu_vmx::SCENE_BASELINE_CONTRACT,
        ),
    );
    match platform::probe_vgpu() {
        Ok(report) => logl::log(
            logl::level::INFO,
            format_args!(
                "HelioV: VMX vGPU ready caps=0x{:016x} epoch={} memory={}/{} queue_timeline={} roundtrip={}",
                report.capabilities,
                report.epoch,
                report.memory_used,
                report.memory_quota,
                report.timeline,
                report.roundtrip_bytes,
            ),
        ),
        Err(failure) => logl::log(
            logl::level::ERROR,
            format_args!(
                "HelioV: VMX vGPU probe failed stage={} rc={}; Helio render remains disabled",
                failure.stage, failure.code,
            ),
        ),
    }
    match wgpu_vmx::probe_wgpu_buffer_path() {
        Ok(bytes) => logl::log(
            logl::level::INFO,
            format_args!("HelioV: WGPU custom Device/Queue buffer path ready roundtrip={bytes}"),
        ),
        Err(failure) => logl::log(
            logl::level::ERROR,
            format_args!(
                "HelioV: WGPU custom buffer path failed stage={} rc={}",
                failure.stage, failure.code,
            ),
        ),
    }
    match scenedb_probe::probe_partner_lifecycle(&world.chunks) {
        Ok(report) => logl::log(
            logl::level::INFO,
            format_args!(
                "HelioV: SceneDB Helio-object world lifecycle ready growth=cpu-shadow-rewrite chunks={} row_span={} reused_row={} flush_ranges={} flush_bytes={} stale_after_despawn={}",
                report.chunk_objects,
                report.row_span,
                report.reused_row,
                report.flush_ranges,
                report.flush_bytes,
                report.stale_after_despawn,
            ),
        ),
        Err(failure) => logl::log(
            logl::level::ERROR,
            format_args!("HelioV: SceneDB partner probe failed invariant={failure}"),
        ),
    }
    match wgpu_vmx::probe_ui4_surface_path(&world) {
        Ok(mut report) => {
            logl::log(
                logl::level::INFO,
                format_args!(
                    "HelioV: Helio material-palette shader/pipeline indexed batch retired and SURFLIVE confirmed extent={}x{} pitch={} bytes={} timeline={} path=Helio-SectionedMeshUpload+SceneDB-world->WGPU-immediates->VMX-resident-scene-batch->UI4-release->SURFLIVE",
                    report.width, report.height, report.pitch, report.bytes, report.timeline,
                ),
            );
            logl::log(
                logl::level::INFO,
                "HelioV: visible material-palette voxel world is live; authenticated immediate-RGBA shader module, graphics pipeline, shared vertex/index bindings, and sectioned draw_indexed batch all executed through the ordinary TRUEOS Render frontier",
            );
            logl::log(
                logl::level::INFO,
                "HelioV: Helio fly camera armed through UI4; click and release the frame once to establish focus, then use primary-drag + WASD + Space/Shift + Ctrl",
            );
            // Retain the proof while already obeying UI4's transactional
            // maximize/restore protocol. The shader milestone replaces the
            // clear in this same loop; it does not introduce another surface.
            loop {
                vsys::poll_once();
                match report.present_pending_resize() {
                    Ok(Some(resize)) => logl::log(
                        logl::level::INFO,
                        format_args!(
                            "HelioV: UI4 resize generation rendered and committed old={}x{} extent={}x{} pitch={} bytes={} timeline={} projection_aspect={:.6} path=resize-event->private-lease->WGPU-submit->UI4-publish",
                            resize.old_width,
                            resize.old_height,
                            resize.width,
                            resize.height,
                            resize.pitch,
                            resize.bytes,
                            resize.timeline,
                            resize.aspect,
                        ),
                    ),
                    Ok(None) => {}
                    Err(failure) => {
                        logl::log(
                            logl::level::ERROR,
                            format_args!(
                                "HelioV: transactional UI4 resize failed stage={} rc={}; previous SURFLIVE generation was not replaced",
                                failure.stage, failure.code,
                            ),
                        );
                        return;
                    }
                }
                match report.present_pending_input() {
                    Ok(Some(input)) => logl::log(
                        logl::level::INFO,
                        format_args!(
                            "HelioV: UI4-routed Helio fly camera live position=({:.3},{:.3},{:.3}) yaw={:.3} pitch={:.3} timeline={} controls=primary-drag+WASD+Space/Shift+Ctrl",
                            input.position[0],
                            input.position[1],
                            input.position[2],
                            input.yaw,
                            input.pitch,
                            input.timeline,
                        ),
                    ),
                    Ok(None) => {}
                    Err(failure) => {
                        logl::log(
                            logl::level::ERROR,
                            format_args!(
                                "HelioV: UI4-routed Helio camera failed stage={} rc={}",
                                failure.stage, failure.code,
                            ),
                        );
                        return;
                    }
                }
                vsys::sleep_ms(16);
            }
        }
        Err(failure) => logl::log(
            logl::level::ERROR,
            format_args!(
                "HelioV: UI4 WGPU render-target probe failed stage={} rc={}",
                failure.stage, failure.code,
            ),
        ),
    }
    logl::log(
        logl::level::ERROR,
        "HelioV: indexed WGPU presentation stopped before the SURFLIVE event loop",
    );
}
