#[cfg(target_os = "trueos")]
mod native {
    const WINGS_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-tier-width-wings.apshader").len();
    const HISTORY_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-smoke-history.apshader").len();
    const CLOUD_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-cloud-history.apshader").len();
    const CLOUD_TRAIL_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-cloud-trail.apshader").len();
    const UNTEXTURED_HISTORY_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-history.apshader").len();
    const FILLED_WINGS_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-translucent-wings.apshader").len();
    const INSET_OUTLINE_BYTES: usize =
        include_bytes!("../shaders/trueos/bfly-inset-outline.apshader").len();
    const MASK_BYTES: usize = 512 * 512;

    #[repr(C)]
    struct MaskStage {
        kind: [u8; 4],
        size: [u8; 4],
        pixels: [u8; MASK_BYTES],
    }

    #[repr(C)]
    struct CelestialMasks {
        magic: [u8; 8],
        id: [u8; 4],
        payload_bytes: [u8; 4],
        stage_count: [u8; 4],
        stages: [MaskStage; 2],
    }

    #[repr(C)]
    struct ShaderPackages {
        wings: [u8; WINGS_BYTES],
        history: [u8; HISTORY_BYTES],
        cloud: [u8; CLOUD_BYTES],
        cloud_trail: [u8; CLOUD_TRAIL_BYTES],
        untextured_history: [u8; UNTEXTURED_HISTORY_BYTES],
        filled_wings: [u8; FILLED_WINGS_BYTES],
        inset_outline: [u8; INSET_OUTLINE_BYTES],
        celestial_masks: CelestialMasks,
    }

    // The Blueprint packer preserves this section as non-ALLOC loader metadata.
    #[used]
    #[unsafe(link_section = ".trueos.shaders")]
    static APP_SHADER_PACKAGES: ShaderPackages = ShaderPackages {
        wings: *include_bytes!("../shaders/trueos/bfly-tier-width-wings.apshader"),
        history: *include_bytes!("../shaders/trueos/bfly-smoke-history.apshader"),
        cloud: *include_bytes!("../shaders/trueos/bfly-cloud-history.apshader"),
        cloud_trail: *include_bytes!("../shaders/trueos/bfly-cloud-trail.apshader"),
        untextured_history: *include_bytes!("../shaders/trueos/bfly-history.apshader"),
        filled_wings: *include_bytes!("../shaders/trueos/bfly-translucent-wings.apshader"),
        inset_outline: *include_bytes!("../shaders/trueos/bfly-inset-outline.apshader"),
        celestial_masks: CelestialMasks {
            magic: *b"APSHDR1\0",
            id: 35u32.to_le_bytes(),
            payload_bytes: (4 + 2 * (8 + MASK_BYTES as u32)).to_le_bytes(),
            stage_count: 2u32.to_le_bytes(),
            stages: [
                MaskStage {
                    kind: 0u32.to_le_bytes(),
                    size: (MASK_BYTES as u32).to_le_bytes(),
                    pixels: *include_bytes!("../shaders/trueos/bfly-moon.r8"),
                },
                MaskStage {
                    kind: 0u32.to_le_bytes(),
                    size: (MASK_BYTES as u32).to_le_bytes(),
                    pixels: *include_bytes!("../shaders/trueos/bfly-earth.r8"),
                },
            ],
        },
    };
}
