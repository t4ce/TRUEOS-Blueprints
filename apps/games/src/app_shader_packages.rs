// Canonical Cubes patch pipeline, authenticated by the host before Games runs.
// The packer keeps this metadata outside the guest's allocated ELF sections.
#[cfg(target_os = "trueos")]
#[used]
#[unsafe(link_section = ".trueos.shaders")]
static CUBE_PATCH_SHADER: [u8; include_bytes!("../assets/cube-patch.apshader").len()] =
    *include_bytes!("../assets/cube-patch.apshader");
