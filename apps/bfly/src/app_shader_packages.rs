#[cfg(target_os = "trueos")]
mod native {
    const WINGS_BYTES: usize = include_bytes!("../shaders/trueos/bfly-direct-wings.apshader").len();
    const HISTORY_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-smoke-history.apshader").len();
    const CLOUD_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-cloud-history.apshader").len();
    const CLOUD_TRAIL_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-cloud-trail.apshader").len();
    const UNTEXTURED_WINGS_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-untextured-wings.apshader").len();
    const UNTEXTURED_HISTORY_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-history.apshader").len();

    #[repr(C)]
    struct ShaderPackages {
        wings: [u8; WINGS_BYTES],
        history: [u8; HISTORY_BYTES],
        cloud: [u8; CLOUD_BYTES],
        cloud_trail: [u8; CLOUD_TRAIL_BYTES],
        untextured_wings: [u8; UNTEXTURED_WINGS_BYTES],
        untextured_history: [u8; UNTEXTURED_HISTORY_BYTES],
    }

    // The Blueprint packer preserves this section as non-ALLOC loader metadata.
    #[used]
    #[unsafe(link_section = ".trueos.shaders")]
    static APP_SHADER_PACKAGES: ShaderPackages = ShaderPackages {
        wings: *include_bytes!("../shaders/trueos/bfly-direct-wings.apshader"),
        history: *include_bytes!("../shaders/trueos/bfly-smoke-history.apshader"),
        cloud: *include_bytes!("../shaders/trueos/bfly-cloud-history.apshader"),
        cloud_trail: *include_bytes!("../shaders/trueos/bfly-cloud-trail.apshader"),
        untextured_wings: *include_bytes!("../shaders/trueos/bfly-untextured-wings.apshader"),
        untextured_history: *include_bytes!("../shaders/trueos/bfly-history.apshader"),
    };
}
