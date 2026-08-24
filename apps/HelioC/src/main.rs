//! HelioC: TRUEOS Cloud Engine Blueprint bring-up target.
//!
//! This is the peer to apps/HelioV. The hosted Cloud Engine supplies the exact
//! WGSL workload and visual oracle; HelioC owns only TRUEOS platform plumbing.

mod cloud_contract;
mod platform;
mod ui4_cloud;
mod ui4_input;

use trueos::logl;

fn main() {
    if let Err(invariant) = cloud_contract::validate() {
        logl::log(
            logl::level::ERROR,
            format_args!("HelioC: authored cloud contract rejected invariant={invariant}"),
        );
        return;
    }

    let camera = cloud_contract::reference_camera();
    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioC: real Helio Cloud Engine Blueprint entered volume={}x{}x{} rgba16f_bytes={} pair_bytes={} sim_params={} render_params={} bindings={} commands={} local={}x{}x{} dispatch={}x{}x{} sim_wgsl_sha256={} render_wgsl_sha256={} wgpu_interface={} camera=({:.3},{:.3},{:.3})",
            cloud_contract::VOLUME_WIDTH,
            cloud_contract::VOLUME_HEIGHT,
            cloud_contract::VOLUME_DEPTH,
            cloud_contract::VOLUME_BYTES,
            cloud_contract::VOLUME_PAIR_BYTES,
            core::mem::size_of::<cloud_contract::SimParams>(),
            core::mem::size_of::<cloud_contract::RenderParams>(),
            cloud_contract::BINDINGS.len(),
            cloud_contract::PASS_SCHEDULE.len(),
            cloud_contract::SIMULATION_LOCAL_SIZE[0],
            cloud_contract::SIMULATION_LOCAL_SIZE[1],
            cloud_contract::SIMULATION_LOCAL_SIZE[2],
            cloud_contract::SIMULATION_DISPATCH[0],
            cloud_contract::SIMULATION_DISPATCH[1],
            cloud_contract::SIMULATION_DISPATCH[2],
            cloud_contract::SIMULATION_WGSL_SHA256,
            cloud_contract::RENDER_WGSL_SHA256,
            cloud_contract::custom_device_interface(),
            camera.position().x,
            camera.position().y,
            camera.position().z,
        ),
    );

    let (resources, report) = match platform::RetainedCloudResources::allocate() {
        Ok(result) => result,
        Err(failure) => {
            logl::log(
                logl::level::ERROR,
                format_args!(
                    "HelioC: retained cloud resource admission failed stage={} rc={}; native path remains cold",
                    failure.stage, failure.code,
                ),
            );
            return;
        }
    };
    logl::log(
        logl::level::INFO,
        format_args!(
            "HelioC: retained VMX cloud graph admitted profile={} graph_raw=0x{:016x} caps=0x{:016x} volumes={}/{} expected_each={} pair_bytes={} params={}/{} mapped_bytes={} ping_pong=artifact-lifetime",
            trueos::vgpu::CLOUD_PROFILE_HELIO_ENGINE_V1,
            resources.graph.raw(),
            report.capabilities,
            resources.volume_a.len(),
            resources.volume_b.len(),
            report.volume_bytes,
            report.pair_bytes,
            report.sim_param_bytes,
            report.render_param_bytes,
            report.mapped_bytes,
        ),
    );
    ui4_cloud::run(resources);
}
