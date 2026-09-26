// Fixed-function state shared by the GL provider and the software draw path.
// Include this file after staticgl.rs so it can use the process helpers/types.

const FIXED_GL_FALSE: u32 = 0;
const FIXED_GL_TRUE: u32 = 1;
const FIXED_GL_LIGHT_MODEL_LOCAL_VIEWER: u32 = 0x0b51;
const FIXED_GL_LIGHT_MODEL_TWO_SIDE: u32 = 0x0b52;
const FIXED_GL_LIGHTING: u32 = 0x0b50;
const FIXED_GL_FOG: u32 = 0x0b60;
const FIXED_GL_DEPTH_TEST: u32 = 0x0b71;
const FIXED_GL_CULL_FACE: u32 = 0x0b44;
const FIXED_GL_ALPHA_TEST: u32 = 0x0bc0;
const FIXED_GL_BLEND: u32 = 0x0be2;
const FIXED_GL_SCISSOR_TEST: u32 = 0x0c11;
const FIXED_GL_POLYGON_OFFSET_POINT: u32 = 0x2a01;
const FIXED_GL_POLYGON_OFFSET_LINE: u32 = 0x2a02;
const FIXED_GL_POLYGON_OFFSET_FILL: u32 = 0x8037;
const FIXED_GL_COLOR_MATERIAL: u32 = 0x0b57;
const FIXED_GL_NORMALIZE: u32 = 0x0ba1;
const FIXED_GL_LIGHT0: u32 = 0x4000;
const FIXED_GL_LIGHT7: u32 = 0x4007;
const FIXED_GL_TEXTURE_GEN_S: u32 = 0x0c60;
const FIXED_GL_TEXTURE_GEN_T: u32 = 0x0c61;
const FIXED_GL_TEXTURE_GEN_R: u32 = 0x0c62;
const FIXED_GL_TEXTURE_GEN_Q: u32 = 0x0c63;
const FIXED_GL_BACK: u32 = 0x0405;
const FIXED_GL_FRONT: u32 = 0x0404;
const FIXED_GL_FRONT_AND_BACK: u32 = 0x0408;
const FIXED_GL_CW: u32 = 0x0900;
const FIXED_GL_CCW: u32 = 0x0901;
const FIXED_GL_NEVER: u32 = 0x0200;
const FIXED_GL_LESS: u32 = 0x0201;
const FIXED_GL_EQUAL: u32 = 0x0202;
const FIXED_GL_LEQUAL: u32 = 0x0203;
const FIXED_GL_GREATER: u32 = 0x0204;
const FIXED_GL_NOTEQUAL: u32 = 0x0205;
const FIXED_GL_GEQUAL: u32 = 0x0206;
const FIXED_GL_ALWAYS: u32 = 0x0207;
const FIXED_GL_ZERO: u32 = 0;
const FIXED_GL_ONE: u32 = 1;
const FIXED_GL_SRC_COLOR: u32 = 0x0300;
const FIXED_GL_ONE_MINUS_SRC_COLOR: u32 = 0x0301;
const FIXED_GL_SRC_ALPHA: u32 = 0x0302;
const FIXED_GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
const FIXED_GL_DST_ALPHA: u32 = 0x0304;
const FIXED_GL_ONE_MINUS_DST_ALPHA: u32 = 0x0305;
const FIXED_GL_DST_COLOR: u32 = 0x0306;
const FIXED_GL_ONE_MINUS_DST_COLOR: u32 = 0x0307;
const FIXED_GL_SRC_ALPHA_SATURATE: u32 = 0x0308;
const FIXED_GL_CONSTANT_COLOR: u32 = 0x8001;
const FIXED_GL_ONE_MINUS_CONSTANT_COLOR: u32 = 0x8002;
const FIXED_GL_CONSTANT_ALPHA: u32 = 0x8003;
const FIXED_GL_ONE_MINUS_CONSTANT_ALPHA: u32 = 0x8004;
const FIXED_GL_AMBIENT_AND_DIFFUSE: u32 = 0x1602;
const FIXED_GL_EMISSION: u32 = 0x1600;
const FIXED_GL_SHININESS: u32 = 0x1601;
const FIXED_GL_SPOT_DIRECTION: u32 = 0x1204;
const FIXED_GL_SPOT_EXPONENT: u32 = 0x1205;
const FIXED_GL_SPOT_CUTOFF: u32 = 0x1206;
const FIXED_GL_CONSTANT_ATTENUATION: u32 = 0x1207;
const FIXED_GL_LINEAR_ATTENUATION: u32 = 0x1208;
const FIXED_GL_QUADRATIC_ATTENUATION: u32 = 0x1209;
const FIXED_GL_FOG_INDEX: u32 = 0x0b61;
const FIXED_GL_FOG_DENSITY: u32 = 0x0b62;
const FIXED_GL_FOG_START: u32 = 0x0b63;
const FIXED_GL_FOG_END: u32 = 0x0b64;
const FIXED_GL_FOG_MODE: u32 = 0x0b65;
const FIXED_GL_FOG_COLOR: u32 = 0x0b66;
const FIXED_GL_EXP: u32 = 0x0800;
const FIXED_GL_EXP2: u32 = 0x0801;
const FIXED_GL_LINEAR: u32 = 0x2601;
const FIXED_GL_TEXTURE_GEN_MODE: u32 = 0x2500;
const FIXED_GL_OBJECT_LINEAR: u32 = 0x2401;
const FIXED_GL_EYE_LINEAR: u32 = 0x2400;
const FIXED_GL_SPHERE_MAP: u32 = 0x2402;
const FIXED_GL_NORMAL_MAP: u32 = 0x8511;
const FIXED_GL_REFLECTION_MAP: u32 = 0x8512;
const FIXED_GL_TEXTURE_2D: u32 = 0x0de1;
const FIXED_GL_NORMAL_ARRAY: u32 = 0x8075;
const FIXED_GL_STENCIL_TEST: u32 = 0x0b90;
const FIXED_GL_TEXTURE_1D: u32 = 0x0de0;
const FIXED_GL_LINE_SMOOTH: u32 = 0x0b20;
const FIXED_GL_POINT_SMOOTH: u32 = 0x0b10;
const FIXED_GL_POLYGON_SMOOTH: u32 = 0x0b41;
const FIXED_GL_DITHER: u32 = 0x0bd0;
const FIXED_GL_MULTISAMPLE: u32 = 0x809d;
const FIXED_GL_LINE_STIPPLE: u32 = 0x0b24;
const FIXED_GL_LOGIC_OP: u32 = 0x0bf1;
const FIXED_GL_POLYGON_STIPPLE: u32 = 0x0b42;
const FIXED_GL_AUTO_NORMAL: u32 = 0x0d80;
const FIXED_GL_CLIP_PLANE0: u32 = 0x3000;
const FIXED_GL_CLIP_PLANE5: u32 = 0x3005;
const FIXED_GL_MAP1_COLOR_4: u32 = 0x0d90;
const FIXED_GL_MAP2_COLOR_4: u32 = 0x0db0;
const FIXED_GL_TEXTURE_3D: u32 = 0x806f;
const FIXED_GL_TEXTURE_CUBE_MAP: u32 = 0x8513;
const FIXED_GL_TEXTURE_RECTANGLE: u32 = 0x84f5;
const FIXED_GL_SAMPLE_ALPHA_TO_COVERAGE: u32 = 0x809e;
const FIXED_GL_SAMPLE_ALPHA_TO_ONE: u32 = 0x809f;
const FIXED_GL_SAMPLE_COVERAGE: u32 = 0x80a0;
const FIXED_GL_POINT_SPRITE: u32 = 0x8861;
const FIXED_GL_COLOR_SUM: u32 = 0x8458;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GlFixedLight {
    ambient: [f32; 4],
    diffuse: [f32; 4],
    specular: [f32; 4],
    position_eye: [f32; 4],
    spot_direction_eye: [f32; 3],
    spot_exponent: f32,
    spot_cutoff: f32,
    attenuation: [f32; 3],
}

impl Default for GlFixedLight {
    fn default() -> Self {
        Self {
            ambient: [0.0, 0.0, 0.0, 1.0],
            diffuse: [0.0, 0.0, 0.0, 1.0],
            specular: [0.0, 0.0, 0.0, 1.0],
            position_eye: [0.0, 0.0, 1.0, 0.0],
            spot_direction_eye: [0.0, 0.0, -1.0],
            spot_exponent: 0.0,
            spot_cutoff: 180.0,
            attenuation: [1.0, 0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GlFixedMaterial {
    ambient: [f32; 4],
    diffuse: [f32; 4],
    specular: [f32; 4],
    emission: [f32; 4],
    shininess: f32,
}

impl Default for GlFixedMaterial {
    fn default() -> Self {
        Self {
            ambient: [0.2, 0.2, 0.2, 1.0],
            diffuse: [0.8, 0.8, 0.8, 1.0],
            specular: [0.0, 0.0, 0.0, 1.0],
            emission: [0.0, 0.0, 0.0, 1.0],
            shininess: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GlFixedFog {
    mode: u32,
    density: f32,
    start: f32,
    end: f32,
    color: [f32; 4],
    index: f32,
}

impl Default for GlFixedFog {
    fn default() -> Self {
        Self {
            mode: FIXED_GL_EXP,
            density: 1.0,
            start: 0.0,
            end: 1.0,
            color: [0.0, 0.0, 0.0, 0.0],
            index: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct GlFixedState {
    enabled: u64,
    error: u32,
    lights: [GlFixedLight; 8],
    light_model_ambient: [f32; 4],
    light_model_local_viewer: bool,
    light_model_two_side: bool,
    materials: [GlFixedMaterial; 2],
    fog: GlFixedFog,
    draw_buffer: u32,
    depth_func: u32,
    depth_mask: bool,
    depth_range: [f64; 2],
    alpha_func: u32,
    alpha_ref: f32,
    blend_factors: [u32; 2],
    scissor: [i32; 4],
    scissor_set: bool,
    polygon_offset: [f32; 2],
    color_material_face: u32,
    color_material_mode: u32,
    texgen_mode: [Option<u32>; 4],
    current_normal: [f32; 3],
    normal_array_enabled: bool,
    normal_pointer: Option<GlArrayPointer>,
}

impl Default for GlFixedState {
    fn default() -> Self {
        let mut lights = [GlFixedLight::default(); 8];
        lights[0].diffuse = [1.0, 1.0, 1.0, 1.0];
        lights[0].specular = [1.0, 1.0, 1.0, 1.0];
        Self {
            // GL_DITHER is enabled in the OpenGL 1.1 initial state.
            enabled: 1u64 << 25,
            error: 0,
            lights,
            light_model_ambient: [0.2, 0.2, 0.2, 1.0],
            light_model_local_viewer: false,
            light_model_two_side: false,
            materials: [GlFixedMaterial::default(); 2],
            fog: GlFixedFog::default(),
            draw_buffer: FIXED_GL_BACK,
            depth_func: FIXED_GL_LESS,
            depth_mask: true,
            depth_range: [0.0, 1.0],
            alpha_func: FIXED_GL_ALWAYS,
            alpha_ref: 0.0,
            blend_factors: [FIXED_GL_ONE, FIXED_GL_ZERO],
            scissor: [0, 0, 0, 0],
            scissor_set: false,
            polygon_offset: [0.0, 0.0],
            color_material_face: FIXED_GL_FRONT_AND_BACK,
            color_material_mode: FIXED_GL_AMBIENT_AND_DIFFUSE,
            texgen_mode: [Some(FIXED_GL_EYE_LINEAR); 4],
            current_normal: [0.0, 0.0, 1.0],
            normal_array_enabled: false,
            normal_pointer: None,
        }
    }
}

impl GlFixedState {
    fn capability_slot(cap: u32) -> Option<u32> {
        match cap {
            FIXED_GL_LIGHTING => Some(0),
            FIXED_GL_FOG => Some(1),
            FIXED_GL_DEPTH_TEST => Some(2),
            FIXED_GL_CULL_FACE => Some(3),
            FIXED_GL_ALPHA_TEST => Some(4),
            FIXED_GL_BLEND => Some(5),
            FIXED_GL_SCISSOR_TEST => Some(6),
            FIXED_GL_POLYGON_OFFSET_POINT => Some(7),
            FIXED_GL_POLYGON_OFFSET_LINE => Some(8),
            FIXED_GL_POLYGON_OFFSET_FILL => Some(9),
            FIXED_GL_COLOR_MATERIAL => Some(10),
            FIXED_GL_NORMALIZE => Some(11),
            FIXED_GL_LIGHT0..=FIXED_GL_LIGHT7 => Some(12 + cap - FIXED_GL_LIGHT0),
            FIXED_GL_TEXTURE_GEN_S..=FIXED_GL_TEXTURE_GEN_Q => {
                Some(20 + cap - FIXED_GL_TEXTURE_GEN_S)
            }
            FIXED_GL_TEXTURE_2D => Some(24),
            FIXED_GL_DITHER => Some(25),
            _ => None,
        }
    }

    fn set_error(&mut self, error: u32) {
        if self.error == 0 {
            self.error = error;
        }
    }

    fn is_valid_unmodeled_capability(cap: u32) -> bool {
        matches!(
            cap,
            FIXED_GL_STENCIL_TEST
                | FIXED_GL_TEXTURE_1D
                | FIXED_GL_LINE_SMOOTH
                | FIXED_GL_POINT_SMOOTH
                | FIXED_GL_POLYGON_SMOOTH
                | FIXED_GL_MULTISAMPLE
                | FIXED_GL_LINE_STIPPLE
                | FIXED_GL_LOGIC_OP
                | FIXED_GL_POLYGON_STIPPLE
                | FIXED_GL_AUTO_NORMAL
                | FIXED_GL_TEXTURE_3D
                | FIXED_GL_TEXTURE_CUBE_MAP
                | FIXED_GL_TEXTURE_RECTANGLE
                | FIXED_GL_SAMPLE_ALPHA_TO_COVERAGE
                | FIXED_GL_SAMPLE_ALPHA_TO_ONE
                | FIXED_GL_SAMPLE_COVERAGE
                | FIXED_GL_POINT_SPRITE
                | FIXED_GL_COLOR_SUM
        ) || (FIXED_GL_CLIP_PLANE0..=FIXED_GL_CLIP_PLANE5).contains(&cap)
            || (FIXED_GL_MAP1_COLOR_4..=0x0d98).contains(&cap)
            || (FIXED_GL_MAP2_COLOR_4..=0x0db8).contains(&cap)
    }

    fn set_enabled(&mut self, cap: u32, enabled: bool) -> Result<(), String> {
        let slot =
            Self::capability_slot(cap).ok_or_else(|| format!("unknown capability=0x{cap:08x}"))?;
        let bit = 1u64 << slot;
        if enabled {
            self.enabled |= bit;
        } else {
            self.enabled &= !bit;
        }
        Ok(())
    }

    fn is_enabled(&self, cap: u32) -> bool {
        Self::capability_slot(cap).is_some_and(|slot| self.enabled & (1u64 << slot) != 0)
    }

    fn set_draw_buffer(&mut self, mode: u32) -> Result<(), String> {
        if mode != FIXED_GL_BACK {
            return Err(format!("unsupported draw buffer=0x{mode:08x}"));
        }
        self.draw_buffer = mode;
        Ok(())
    }

    fn set_depth_func(&mut self, func: u32) -> Result<(), String> {
        if !matches!(
            func,
            FIXED_GL_NEVER
                | FIXED_GL_LESS
                | FIXED_GL_EQUAL
                | FIXED_GL_LEQUAL
                | FIXED_GL_GREATER
                | FIXED_GL_NOTEQUAL
                | FIXED_GL_GEQUAL
                | FIXED_GL_ALWAYS
        ) {
            return Err(format!("unknown depth func=0x{func:08x}"));
        }
        self.depth_func = func;
        Ok(())
    }

    fn set_depth_range(&mut self, near: f64, far: f64) -> Result<(), String> {
        if !near.is_finite() || !far.is_finite() {
            return Err("invalid value: nonfinite depth range".into());
        }
        self.depth_range = [near.clamp(0.0, 1.0), far.clamp(0.0, 1.0)];
        Ok(())
    }

    fn set_blend_func(&mut self, source: u32, destination: u32) -> Result<(), String> {
        if !valid_blend_factor(source)
            || !valid_blend_factor(destination)
            || destination == FIXED_GL_SRC_ALPHA_SATURATE
        {
            return Err(format!(
                "unknown blend factors 0x{source:08x}/0x{destination:08x}"
            ));
        }
        self.blend_factors = [source, destination];
        Ok(())
    }

    fn set_alpha_func(&mut self, func: u32, reference: f32) -> Result<(), String> {
        if !valid_compare(func) {
            return Err(format!("unknown alpha func=0x{func:08x}"));
        }
        if !reference.is_finite() {
            return Err("invalid value: nonfinite alpha reference".into());
        }
        self.alpha_func = func;
        self.alpha_ref = reference.clamp(0.0, 1.0);
        Ok(())
    }

    fn set_scissor(&mut self, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
        if width < 0 || height < 0 {
            return Err(format!(
                "invalid value: negative scissor size={width}x{height}"
            ));
        }
        self.scissor = [x, y, width, height];
        self.scissor_set = true;
        Ok(())
    }

    fn set_polygon_offset(&mut self, factor: f32, units: f32) -> Result<(), String> {
        if !factor.is_finite() || !units.is_finite() {
            return Err("invalid value: nonfinite polygon offset".into());
        }
        self.polygon_offset = [factor, units];
        Ok(())
    }

    fn set_texgen_mode(&mut self, coordinate: u32, pname: u32, mode: u32) -> Result<(), String> {
        let slot = match coordinate {
            0x2000 => 0,
            0x2001 => 1,
            0x2002 => 2,
            0x2003 => 3,
            _ => return Err(format!("unknown texgen coordinate=0x{coordinate:08x}")),
        };
        if pname != FIXED_GL_TEXTURE_GEN_MODE {
            return Err(format!("unknown texgen pname=0x{pname:08x}"));
        }
        if !matches!(
            mode,
            FIXED_GL_OBJECT_LINEAR
                | FIXED_GL_EYE_LINEAR
                | FIXED_GL_SPHERE_MAP
                | FIXED_GL_NORMAL_MAP
                | FIXED_GL_REFLECTION_MAP
        ) {
            return Err(format!("unknown texgen mode=0x{mode:08x}"));
        }
        self.texgen_mode[slot] = Some(mode);
        Ok(())
    }

    fn set_color_material(&mut self, face: u32, mode: u32) -> Result<(), String> {
        if !matches!(
            face,
            FIXED_GL_FRONT | FIXED_GL_BACK | FIXED_GL_FRONT_AND_BACK
        ) {
            return Err(format!("unknown color-material face=0x{face:08x}"));
        }
        if !matches!(
            mode,
            GL_AMBIENT
                | GL_DIFFUSE
                | GL_SPECULAR
                | FIXED_GL_EMISSION
                | FIXED_GL_AMBIENT_AND_DIFFUSE
        ) {
            return Err(format!("unknown color-material mode=0x{mode:08x}"));
        }
        self.color_material_face = face;
        self.color_material_mode = mode;
        Ok(())
    }

    fn set_fog_value(&mut self, pname: u32, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err("invalid value: nonfinite fog value".into());
        }
        match pname {
            FIXED_GL_FOG_DENSITY if value >= 0.0 => self.fog.density = value,
            FIXED_GL_FOG_DENSITY => return Err("invalid value: negative fog density".into()),
            FIXED_GL_FOG_START => self.fog.start = value,
            FIXED_GL_FOG_END => self.fog.end = value,
            FIXED_GL_FOG_INDEX => self.fog.index = value,
            FIXED_GL_FOG_MODE => {
                if value < 0.0
                    || value.fract() != 0.0
                    || !matches!(value as u32, FIXED_GL_EXP | FIXED_GL_EXP2 | FIXED_GL_LINEAR)
                {
                    return Err("unknown fog mode enum".into());
                }
                self.fog.mode = value as u32;
            }
            _ => return Err(format!("unknown fog pname=0x{pname:08x}")),
        }
        Ok(())
    }

    fn set_fog_fv(&mut self, pname: u32, values: &[f32]) -> Result<(), String> {
        if values.iter().any(|v| !v.is_finite()) {
            return Err("invalid value: nonfinite fog value".into());
        }
        if pname == FIXED_GL_FOG_COLOR && values.len() == 4 {
            self.fog.color.copy_from_slice(values);
            return Ok(());
        }
        if values.len() == 1 {
            return self.set_fog_value(pname, values[0]);
        }
        Err(format!(
            "unknown fog pname/arity=0x{pname:08x}/{}",
            values.len()
        ))
    }

    fn set_fog_i(&mut self, pname: u32, value: i32) -> Result<(), String> {
        if pname == FIXED_GL_FOG_MODE {
            if !matches!(value as u32, FIXED_GL_EXP | FIXED_GL_EXP2 | FIXED_GL_LINEAR) {
                return Err("unknown fog mode enum".into());
            }
            self.fog.mode = value as u32;
            return Ok(());
        }
        self.set_fog_value(pname, value as f32)
    }

    fn set_material(&mut self, face: u32, pname: u32, values: &[f32]) -> Result<(), String> {
        if values.iter().any(|v| !v.is_finite()) {
            return Err("invalid value: nonfinite material value".into());
        }
        if pname == FIXED_GL_SHININESS && values.len() == 1 && !(0.0..=128.0).contains(&values[0]) {
            return Err("invalid value: material shininess outside 0..128".into());
        }
        let faces: &[usize] = match face {
            FIXED_GL_FRONT => &[0],
            FIXED_GL_BACK => &[1],
            FIXED_GL_FRONT_AND_BACK => &[0, 1],
            _ => return Err(format!("unknown material face=0x{face:08x}")),
        };
        for &index in faces {
            let material = &mut self.materials[index];
            match pname {
                GL_AMBIENT if values.len() == 4 => material.ambient.copy_from_slice(values),
                GL_DIFFUSE if values.len() == 4 => material.diffuse.copy_from_slice(values),
                GL_SPECULAR if values.len() == 4 => material.specular.copy_from_slice(values),
                FIXED_GL_EMISSION if values.len() == 4 => material.emission.copy_from_slice(values),
                FIXED_GL_AMBIENT_AND_DIFFUSE if values.len() == 4 => {
                    material.ambient.copy_from_slice(values);
                    material.diffuse.copy_from_slice(values);
                }
                FIXED_GL_SHININESS if values.len() == 1 && (0.0..=128.0).contains(&values[0]) => {
                    material.shininess = values[0]
                }
                _ => {
                    return Err(format!(
                        "unknown material pname/arity=0x{pname:08x}/{}",
                        values.len()
                    ))
                }
            }
        }
        Ok(())
    }
}

fn valid_compare(value: u32) -> bool {
    matches!(
        value,
        FIXED_GL_NEVER
            | FIXED_GL_LESS
            | FIXED_GL_EQUAL
            | FIXED_GL_LEQUAL
            | FIXED_GL_GREATER
            | FIXED_GL_NOTEQUAL
            | FIXED_GL_GEQUAL
            | FIXED_GL_ALWAYS
    )
}
fn valid_blend_factor(value: u32) -> bool {
    matches!(
        value,
        FIXED_GL_ZERO
            | FIXED_GL_ONE
            | FIXED_GL_SRC_COLOR
            | FIXED_GL_ONE_MINUS_SRC_COLOR
            | FIXED_GL_SRC_ALPHA
            | FIXED_GL_ONE_MINUS_SRC_ALPHA
            | FIXED_GL_DST_ALPHA
            | FIXED_GL_ONE_MINUS_DST_ALPHA
            | FIXED_GL_DST_COLOR
            | FIXED_GL_ONE_MINUS_DST_COLOR
            | FIXED_GL_SRC_ALPHA_SATURATE
            | FIXED_GL_CONSTANT_COLOR
            | FIXED_GL_ONE_MINUS_CONSTANT_COLOR
            | FIXED_GL_CONSTANT_ALPHA
            | FIXED_GL_ONE_MINUS_CONSTANT_ALPHA
    )
}

fn gl_inverse_transpose_direction(
    matrix: &[f32; 16],
    direction: [f32; 3],
) -> Result<[f32; 3], &'static str> {
    // Inverse transpose of the upper-left 3x3, as required for GL spot directions.
    let a = [
        [matrix[0], matrix[4], matrix[8]],
        [matrix[1], matrix[5], matrix[9]],
        [matrix[2], matrix[6], matrix[10]],
    ];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !det.is_finite() || det.abs() <= f32::EPSILON {
        return Err("singular modelview for spot direction");
    }
    let inv_det = 1.0 / det;
    let inverse = [
        [
            (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * inv_det,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * inv_det,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * inv_det,
        ],
        [
            (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * inv_det,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * inv_det,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * inv_det,
        ],
        [
            (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * inv_det,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * inv_det,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * inv_det,
        ],
    ];
    Ok(core::array::from_fn(|row| {
        (0..3).map(|col| inverse[col][row] * direction[col]).sum()
    }))
}

impl XpProcess {
    fn fixed_state_mut(
        &mut self,
        tid: u32,
        api: &'static str,
    ) -> Result<&mut WglContext, ProviderDispatchError> {
        self.gl_context_mut(tid, api)
    }
    fn fixed_error(api: &'static str, tid: u32, error: String) -> ProviderDispatchError {
        ProviderDispatchError::Frontier {
            api,
            detail: format!("tid={tid} {error}"),
        }
    }

    fn fixed_latch_error(&mut self, tid: u32, detail: &str) {
        if let Ok(context) = self.gl_context_mut(tid, "OpenGL fixed state") {
            let code = if detail.starts_with("unknown") || detail.starts_with("unsupported") {
                0x0500 // GL_INVALID_ENUM
            } else {
                0x0501 // GL_INVALID_VALUE
            };
            context.fixed.set_error(code);
        }
    }

    fn gl_disable_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, cap] = arguments::<2>(memory, esp)?;
        if GlFixedState::is_valid_unmodeled_capability(cap) {
            return Err(Self::fixed_error(
                "glDisable",
                tid,
                format!("valid but unmodeled capability=0x{cap:08x}"),
            ));
        }
        let context = self.fixed_state_mut(tid, "glDisable")?;
        if cap == FIXED_GL_TEXTURE_2D {
            context.textures.enabled = false;
        }
        if context.fixed.set_enabled(cap, false).is_err() {
            context.fixed.set_error(0x0500);
        }
        if cap == FIXED_GL_LIGHT0 {
            context.light0_enabled = false;
        }
        Ok(0)
    }

    fn gl_enable_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, cap] = arguments::<2>(memory, esp)?;
        if GlFixedState::is_valid_unmodeled_capability(cap) {
            return Err(Self::fixed_error(
                "glEnable",
                tid,
                format!("valid but unmodeled capability=0x{cap:08x}"),
            ));
        }
        let context = self.fixed_state_mut(tid, "glEnable")?;
        if cap == FIXED_GL_TEXTURE_2D {
            context.textures.enabled = true;
        }
        if context.fixed.set_enabled(cap, true).is_err() {
            context.fixed.set_error(0x0500);
        }
        if cap == FIXED_GL_LIGHT0 {
            context.light0_enabled = true;
        }
        Ok(0)
    }

    fn gl_fixed_client_state_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
        enabled: bool,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, array] = arguments::<2>(memory, esp)?;
        let context = self.fixed_state_mut(
            tid,
            if enabled {
                "glEnableClientState"
            } else {
                "glDisableClientState"
            },
        )?;
        match array {
            GL_VERTEX_ARRAY => context.vertex_array_enabled = enabled,
            GL_COLOR_ARRAY => context.color_array_enabled = enabled,
            GL_TEXTURE_COORD_ARRAY => context.textures.coord_array_enabled = enabled,
            FIXED_GL_NORMAL_ARRAY => context.fixed.normal_array_enabled = enabled,
            _ => {
                return Err(Self::fixed_error(
                    if enabled {
                        "glEnableClientState"
                    } else {
                        "glDisableClientState"
                    },
                    tid,
                    format!("unknown array=0x{array:08x}"),
                ))
            }
        }
        Ok(0)
    }

    fn gl_lightfv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, light, pname, params] = arguments::<4>(memory, esp)?;
        if params == 0 {
            self.fixed_latch_error(tid, "invalid value: null params");
            return Ok(0);
        }
        let index = match light.checked_sub(FIXED_GL_LIGHT0).filter(|n| *n < 8) {
            Some(n) => n as usize,
            None => {
                self.fixed_latch_error(tid, "unknown light enum");
                return Ok(0);
            }
        };
        let count = match pname {
            GL_AMBIENT | GL_DIFFUSE | GL_SPECULAR | GL_POSITION => 4,
            FIXED_GL_SPOT_DIRECTION => 3,
            FIXED_GL_SPOT_EXPONENT
            | FIXED_GL_SPOT_CUTOFF
            | FIXED_GL_CONSTANT_ATTENUATION
            | FIXED_GL_LINEAR_ATTENUATION
            | FIXED_GL_QUADRATIC_ATTENUATION => 1,
            _ => {
                self.fixed_latch_error(tid, "unknown light pname enum");
                return Ok(0);
            }
        };
        let mut raw = vec![0u8; count * 4];
        memory.read(params, &mut raw)?;
        let values: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        if values.iter().any(|v| !v.is_finite()) {
            self.fixed_latch_error(tid, "invalid value: nonfinite light value");
            return Ok(0);
        }
        let value_valid = match pname {
            FIXED_GL_SPOT_EXPONENT => (0.0..=128.0).contains(&values[0]),
            FIXED_GL_SPOT_CUTOFF => values[0] == 180.0 || (0.0..=90.0).contains(&values[0]),
            FIXED_GL_CONSTANT_ATTENUATION
            | FIXED_GL_LINEAR_ATTENUATION
            | FIXED_GL_QUADRATIC_ATTENUATION => values[0] >= 0.0,
            _ => true,
        };
        if !value_valid {
            self.fixed_latch_error(tid, "invalid value: light parameter out of range");
            return Ok(0);
        }
        let modelview = self.fixed_state_mut(tid, "glLightfv")?.modelview_matrix;
        let transformed_spot = if pname == FIXED_GL_SPOT_DIRECTION {
            match gl_inverse_transpose_direction(&modelview, [values[0], values[1], values[2]]) {
                Ok(v) => Some(v),
                Err(_) => {
                    self.fixed_latch_error(
                        tid,
                        "invalid value: singular modelview for spot direction",
                    );
                    return Ok(0);
                }
            }
        } else {
            None
        };
        let context = self.fixed_state_mut(tid, "glLightfv")?;
        let light_state = &mut context.fixed.lights[index];
        match pname {
            GL_AMBIENT => light_state.ambient.copy_from_slice(&values),
            GL_DIFFUSE => light_state.diffuse.copy_from_slice(&values),
            GL_SPECULAR => light_state.specular.copy_from_slice(&values),
            GL_POSITION => {
                light_state.position_eye =
                    gl_transform(&modelview, [values[0], values[1], values[2], values[3]])
            }
            FIXED_GL_SPOT_DIRECTION => light_state.spot_direction_eye = transformed_spot.unwrap(),
            FIXED_GL_SPOT_EXPONENT => light_state.spot_exponent = values[0],
            FIXED_GL_SPOT_CUTOFF => light_state.spot_cutoff = values[0],
            FIXED_GL_CONSTANT_ATTENUATION => light_state.attenuation[0] = values[0],
            FIXED_GL_LINEAR_ATTENUATION => light_state.attenuation[1] = values[0],
            FIXED_GL_QUADRATIC_ATTENUATION => light_state.attenuation[2] = values[0],
            _ => unreachable!(),
        }
        if index == 0 {
            context.light0_ambient = light_state.ambient;
            context.light0_diffuse = light_state.diffuse;
            context.light0_specular = light_state.specular;
            context.light0_position_eye = light_state.position_eye;
        }
        Ok(0)
    }

    fn gl_lightf_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, light, pname, bits] = arguments::<4>(memory, esp)?;
        let value = f32::from_bits(bits);
        let index = match light.checked_sub(FIXED_GL_LIGHT0).filter(|n| *n < 8) {
            Some(n) => n as usize,
            None => {
                self.fixed_latch_error(tid, "unknown light enum");
                return Ok(0);
            }
        };
        if !matches!(
            pname,
            FIXED_GL_SPOT_EXPONENT
                | FIXED_GL_SPOT_CUTOFF
                | FIXED_GL_CONSTANT_ATTENUATION
                | FIXED_GL_LINEAR_ATTENUATION
                | FIXED_GL_QUADRATIC_ATTENUATION
        ) {
            self.fixed_latch_error(tid, "unknown light pname enum");
            return Ok(0);
        }
        if !value.is_finite() {
            self.fixed_latch_error(tid, "invalid value: nonfinite light parameter");
            return Ok(0);
        }
        let valid = match pname {
            FIXED_GL_SPOT_EXPONENT => (0.0..=128.0).contains(&value),
            FIXED_GL_SPOT_CUTOFF => value == 180.0 || (0.0..=90.0).contains(&value),
            _ => value >= 0.0,
        };
        if !valid {
            self.fixed_latch_error(tid, "invalid value: light parameter out of range");
            return Ok(0);
        }
        let state = &mut self.fixed_state_mut(tid, "glLightf")?.fixed.lights[index];
        match pname {
            FIXED_GL_SPOT_EXPONENT => state.spot_exponent = value,
            FIXED_GL_SPOT_CUTOFF => state.spot_cutoff = value,
            FIXED_GL_CONSTANT_ATTENUATION => state.attenuation[0] = value,
            FIXED_GL_LINEAR_ATTENUATION => state.attenuation[1] = value,
            FIXED_GL_QUADRATIC_ATTENUATION => state.attenuation[2] = value,
            _ => unreachable!(),
        }
        Ok(0)
    }

    fn gl_light_modelfv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, params] = arguments::<3>(memory, esp)?;
        let count = if pname == GL_LIGHT_MODEL_AMBIENT {
            4
        } else if matches!(
            pname,
            FIXED_GL_LIGHT_MODEL_LOCAL_VIEWER | FIXED_GL_LIGHT_MODEL_TWO_SIDE
        ) {
            1
        } else {
            self.fixed_latch_error(tid, "unknown light model pname");
            return Ok(0);
        };
        if params == 0 {
            self.fixed_latch_error(tid, "invalid value: null params");
            return Ok(0);
        }
        let mut bytes = vec![0; count * 4];
        memory.read(params, &mut bytes)?;
        let values: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        if values.iter().any(|v| !v.is_finite()) {
            self.fixed_latch_error(tid, "invalid value: nonfinite light model value");
            return Ok(0);
        }
        let context = self.fixed_state_mut(tid, "glLightModelfv")?;
        match pname {
            GL_LIGHT_MODEL_AMBIENT => {
                let v: [f32; 4] = values.try_into().unwrap();
                context.fixed.light_model_ambient = v;
                context.light_model_ambient = v;
            }
            FIXED_GL_LIGHT_MODEL_LOCAL_VIEWER => {
                context.fixed.light_model_local_viewer = values[0] != 0.0
            }
            FIXED_GL_LIGHT_MODEL_TWO_SIDE => context.fixed.light_model_two_side = values[0] != 0.0,
            _ => unreachable!(),
        }
        Ok(0)
    }

    fn gl_materialfv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, face, pname, params] = arguments::<4>(memory, esp)?;
        if params == 0 {
            return Err(Self::fixed_error("glMaterialfv", tid, "null params".into()));
        }
        let count = if pname == FIXED_GL_SHININESS { 1 } else { 4 };
        let mut raw = vec![0; count * 4];
        memory.read(params, &mut raw)?;
        let values: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let result = self
            .fixed_state_mut(tid, "glMaterialfv")?
            .fixed
            .set_material(face, pname, &values);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }

    fn gl_color_material_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, face, mode] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glColorMaterial")?
            .fixed
            .set_color_material(face, mode);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }

    fn gl_fogfv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, params] = arguments::<3>(memory, esp)?;
        if params == 0 {
            self.fixed_latch_error(tid, "invalid value: null params");
            return Ok(0);
        }
        let count = if pname == FIXED_GL_FOG_COLOR { 4 } else { 1 };
        let mut raw = vec![0; count * 4];
        memory.read(params, &mut raw)?;
        let vals: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let result = self
            .fixed_state_mut(tid, "glFogfv")?
            .fixed
            .set_fog_fv(pname, &vals);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_fogf_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, bits] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glFogf")?
            .fixed
            .set_fog_value(pname, f32::from_bits(bits));
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_fogi_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, value] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glFogi")?
            .fixed
            .set_fog_i(pname, value as i32);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_draw_buffer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mode] = arguments::<2>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glDrawBuffer")?
            .fixed
            .set_draw_buffer(mode);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_depth_func_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, func] = arguments::<2>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glDepthFunc")?
            .fixed
            .set_depth_func(func);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_depth_mask_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, flag] = arguments::<2>(memory, esp)?;
        let context = self.fixed_state_mut(tid, "glDepthMask")?;
        context.fixed.depth_mask = flag != 0;
        Ok(0)
    }
    fn gl_depth_range_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, near_lo, near_hi, far_lo, far_hi] = arguments::<5>(memory, esp)?;
        let near = f64::from_bits(u64::from(near_lo) | (u64::from(near_hi) << 32));
        let far = f64::from_bits(u64::from(far_lo) | (u64::from(far_hi) << 32));
        let result = self
            .fixed_state_mut(tid, "glDepthRange")?
            .fixed
            .set_depth_range(near, far);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_alpha_func_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, func, bits] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glAlphaFunc")?
            .fixed
            .set_alpha_func(func, f32::from_bits(bits));
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_blend_func_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, src, dst] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glBlendFunc")?
            .fixed
            .set_blend_func(src, dst);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_scissor_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, x, y, w, h] = arguments::<5>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glScissor")?
            .fixed
            .set_scissor(x as i32, y as i32, w as i32, h as i32);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_polygon_offset_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, f, u] = arguments::<3>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glPolygonOffset")?
            .fixed
            .set_polygon_offset(f32::from_bits(f), f32::from_bits(u));
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }
    fn gl_tex_geni_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, coord, pname, value] = arguments::<4>(memory, esp)?;
        let result = self
            .fixed_state_mut(tid, "glTexGeni")?
            .fixed
            .set_texgen_mode(coord, pname, value as u32);
        if let Err(e) = result {
            self.fixed_latch_error(tid, &e);
        }
        Ok(0)
    }

    fn gl_normal_3fv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, ptr] = arguments::<2>(memory, esp)?;
        if ptr == 0 {
            self.fixed_latch_error(tid, "invalid value: null normal pointer");
            return Ok(0);
        }
        let mut raw = [0; 12];
        memory.read(ptr, &mut raw)?;
        let n =
            core::array::from_fn(|i| f32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap()));
        if n.iter().any(|v| !v.is_finite()) {
            self.fixed_latch_error(tid, "invalid value: nonfinite normal");
            return Ok(0);
        }
        self.fixed_state_mut(tid, "glNormal3fv")?
            .fixed
            .current_normal = n;
        Ok(0)
    }
    fn gl_normal_pointer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, kind, stride, address] = arguments::<4>(memory, esp)?;
        if kind != GL_FLOAT || (stride as i32) < 0 {
            return Err(Self::fixed_error(
                "glNormalPointer",
                tid,
                format!("unsupported type/stride=0x{kind:08x}/{}", stride as i32),
            ));
        }
        let context = self.fixed_state_mut(tid, "glNormalPointer")?;
        context.fixed.normal_pointer = Some(GlArrayPointer {
            size: 3,
            kind,
            stride,
            address,
        });
        Ok(0)
    }
}

#[cfg(test)]
mod staticgl_fixed_tests {
    use super::*;
    include!("staticgl_fixed_tests.rs");
}
