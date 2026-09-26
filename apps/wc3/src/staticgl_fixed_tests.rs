#[test]
fn fixed_defaults_match_legacy_gl_and_start_disabled() {
    let state = GlFixedState::default();
    assert_eq!(state.draw_buffer, FIXED_GL_BACK);
    assert_eq!(state.depth_func, FIXED_GL_LESS);
    assert!(state.depth_mask);
    assert_eq!(state.depth_range, [0.0, 1.0]);
    assert!(!state.scissor_set);
    assert_eq!(state.alpha_func, FIXED_GL_ALWAYS);
    assert_eq!(state.blend_factors, [FIXED_GL_ONE, FIXED_GL_ZERO]);
    assert_eq!(state.lights[0].position_eye, [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(state.lights[0].diffuse, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(state.lights[1].diffuse, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(state.lights[7].attenuation, [1.0, 0.0, 0.0]);
    assert!(!state.is_enabled(FIXED_GL_LIGHTING));
    assert!(!state.is_enabled(FIXED_GL_LIGHT7));
    assert_eq!(state.error, 0);
}

#[test]
fn fixed_state_transitions_preserve_values_and_independent_light_slots() {
    let mut state = GlFixedState::default();
    state.set_enabled(FIXED_GL_LIGHTING, true).unwrap();
    state.set_enabled(FIXED_GL_LIGHT3, true).unwrap();
    state.set_enabled(FIXED_GL_BLEND, true).unwrap();
    state.set_draw_buffer(FIXED_GL_BACK).unwrap();
    state.set_depth_func(FIXED_GL_GEQUAL).unwrap();
    state.set_depth_range(-2.0, 0.5).unwrap();
    state.set_alpha_func(FIXED_GL_GREATER, 0.25).unwrap();
    state
        .set_blend_func(FIXED_GL_SRC_ALPHA, FIXED_GL_ONE_MINUS_SRC_ALPHA)
        .unwrap();
    state.set_scissor(12, 15, 640, 480).unwrap();
    state.set_polygon_offset(1.25, -2.0).unwrap();
    state
        .set_color_material(FIXED_GL_FRONT_AND_BACK, FIXED_GL_AMBIENT_AND_DIFFUSE)
        .unwrap();
    state
        .set_texgen_mode(0x2000, FIXED_GL_TEXTURE_GEN_MODE, FIXED_GL_SPHERE_MAP)
        .unwrap();
    state
        .set_material(FIXED_GL_FRONT, FIXED_GL_SHININESS, &[32.0])
        .unwrap();
    state.lights[3].diffuse = [0.1, 0.2, 0.3, 1.0];

    assert!(state.is_enabled(FIXED_GL_LIGHTING));
    assert!(state.is_enabled(FIXED_GL_LIGHT3));
    assert!(!state.is_enabled(FIXED_GL_LIGHT2));
    assert_eq!(state.depth_range, [0.0, 0.5]);
    assert_eq!(state.scissor, [12, 15, 640, 480]);
    assert!(state.scissor_set);
    assert_eq!(state.polygon_offset, [1.25, -2.0]);
    assert_eq!(state.texgen_mode[0], Some(FIXED_GL_SPHERE_MAP));
    assert_eq!(state.materials[0].shininess, 32.0);
    assert_eq!(state.materials[1].shininess, 0.0);
    assert_eq!(state.lights[3].diffuse, [0.1, 0.2, 0.3, 1.0]);
}

#[test]
fn fixed_validation_rejects_unknown_enums_and_bad_values_without_mutation() {
    let mut state = GlFixedState::default();
    let old_scissor = state.scissor;
    assert!(state.set_scissor(1, 2, -1, 8).is_err());
    assert_eq!(state.scissor, old_scissor);
    assert!(!state.scissor_set);
    assert!(state.set_enabled(0xdead, true).is_err());
    assert!(state.set_depth_func(0xdead).is_err());
    assert!(state.set_blend_func(0xdead, FIXED_GL_ONE).is_err());
    assert!(state
        .set_blend_func(FIXED_GL_ONE, FIXED_GL_SRC_ALPHA_SATURATE)
        .is_err());
    assert!(state
        .set_texgen_mode(0x2000, FIXED_GL_TEXTURE_GEN_MODE, 0xdead)
        .is_err());
    assert!(state
        .set_material(FIXED_GL_FRONT, FIXED_GL_SHININESS, &[129.0])
        .is_err());
    assert_eq!(state.materials[0].shininess, 0.0);
}
