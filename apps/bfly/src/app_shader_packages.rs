#[cfg(target_os = "trueos")]
mod native {
    const WINGS_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-neutral-mixed-outline.apshader").len();
    const SUNRISE_RIBBON_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-sunrise-ribbon.apshader").len();
    const SUNRISE_OUTLINE_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-neutral-wake-outline.apshader").len();
    const HISTORY_BYTES: usize = include_bytes!("../shaders/trueos/bfly-history.apshader").len();
    const FILLED_WINGS_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-translucent-wings.apshader").len();
    const INSET_OUTLINE_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-neutral-inset-outline.apshader").len();
    #[repr(C)]
    struct ShaderPackages {
        wings: [u8; WINGS_BYTES],
        sunrise_ribbon: [u8; SUNRISE_RIBBON_BYTES],
        sunrise_outline: [u8; SUNRISE_OUTLINE_BYTES],
        history: [u8; HISTORY_BYTES],
        filled_wings: [u8; FILLED_WINGS_BYTES],
        inset_outline: [u8; INSET_OUTLINE_BYTES],
    }

    // The Blueprint packer preserves this section as non-ALLOC loader metadata.
    #[used]
    #[unsafe(link_section = ".trueos.shaders")]
    static APP_SHADER_PACKAGES: ShaderPackages = ShaderPackages {
        wings: *include_bytes!("../shaders/trueos/bfly-neutral-mixed-outline.apshader"),
        sunrise_ribbon: *include_bytes!("../shaders/trueos/bfly-sunrise-ribbon.apshader"),
        sunrise_outline: *include_bytes!("../shaders/trueos/bfly-neutral-wake-outline.apshader"),
        history: *include_bytes!("../shaders/trueos/bfly-history.apshader"),
        filled_wings: *include_bytes!("../shaders/trueos/bfly-translucent-wings.apshader"),
        inset_outline: *include_bytes!("../shaders/trueos/bfly-neutral-inset-outline.apshader"),
    };
}
