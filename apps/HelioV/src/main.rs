//! HelioV: the native Helio + SceneDB Blueprint bring-up application.
//!
//! This is intentionally not a port of Stratum's deprecated `voxel_world`
//! runtime. The voxel world is only the product target. Geometry is authored
//! directly in Helio types and, once the custom WGPU backend is available,
//! scene lifetime and rendering remain owned by Helio and SceneDB.

mod backend_contract;
mod platform;
mod scenedb_probe;
mod voxel;
mod wgpu_vmx;

use trueos::{logl, vsys};

fn main() {
    let chunk = voxel::build_voxel_chunk();
    let fingerprint = voxel::mesh_fingerprint(&chunk.mesh);

    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioV: real Helio Blueprint entered; voxels={} vertices={} indices={} mesh_fingerprint=0x{fingerprint:016x}",
            chunk.solid_voxels,
            chunk.mesh.vertices.len(),
            chunk.mesh.indices.len(),
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
                failure.stage,
                failure.code,
            ),
        ),
    }
    match wgpu_vmx::probe_wgpu_buffer_path() {
        Ok(bytes) => logl::log(
            logl::level::INFO,
            format_args!(
                "HelioV: WGPU custom Device/Queue buffer path ready roundtrip={bytes}"
            ),
        ),
        Err(failure) => logl::log(
            logl::level::ERROR,
            format_args!(
                "HelioV: WGPU custom buffer path failed stage={} rc={}",
                failure.stage, failure.code,
            ),
        ),
    }
    match scenedb_probe::probe_partner_lifecycle() {
        Ok(report) => logl::log(
            logl::level::INFO,
            format_args!(
                "HelioV: SceneDB Helio-object partner lifecycle ready row={} flush_ranges={} flush_bytes={} stale_after_despawn={}",
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
    match wgpu_vmx::probe_ui4_surface_path() {
        Ok(mut report) => {
            logl::log(
                logl::level::INFO,
                format_args!(
                    "HelioV: WGPU render-pass clear retired and SURFLIVE confirmed extent={}x{} pitch={} bytes={} timeline={} path=wgpu::CommandEncoder->VMX-iGPU->UI4-release->SURFLIVE",
                    report.width, report.height, report.pitch, report.bytes, report.timeline,
                ),
            );
            logl::log(
                logl::level::INFO,
                "HelioV: visible WGPU command submission is live; shader/pipeline and indexed voxel draw remain pending, no alternate renderer is permitted",
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
        "HelioV: WGPU presentation proof failed; shader/pipeline bring-up remains disabled",
    );
}
