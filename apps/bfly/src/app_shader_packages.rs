// The Blueprint packer preserves this section as non-ALLOC loader metadata.
#[cfg(target_os = "trueos")]
#[used]
#[unsafe(link_section = ".trueos.shaders")]
static APP_SHADER_PACKAGES: [u8; include_bytes!("../shaders/trueos/app-shaders.bundle").len()] =
    *include_bytes!("../shaders/trueos/app-shaders.bundle");
