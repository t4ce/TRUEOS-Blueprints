// Guest arrays are decoded into owned vertices before any framebuffer mutation.
// Lighting stays in eye coordinates; homogeneous clip W is preserved for raster.
fn gl_vec_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0]*b[0] + a[1]*b[1] + a[2]*b[2]
}
fn gl_vec_normalize(v: [f32; 3]) -> [f32; 3] {
    let length = gl_vec_dot(v,v).sqrt();
    if length > 0.0 { v.map(|c|c/length) } else { [0.0;3] }
}
fn gl_normal_matrix(matrix: &[f32;16]) -> Result<[[f32;3];3], &'static str> {
    let a=[matrix[0],matrix[1],matrix[2]];
    let b=[matrix[4],matrix[5],matrix[6]];
    let c=[matrix[8],matrix[9],matrix[10]];
    let cross=|u:[f32;3],v:[f32;3]|[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
    let cofactors=[cross(b,c),cross(c,a),cross(a,b)];
    let determinant=gl_vec_dot(a,cofactors[0]);
    if !determinant.is_finite() || determinant.abs() < 1e-20 { return Err("singular normal transform"); }
    Ok(cofactors.map(|v|v.map(|x|x/determinant)))
}
fn gl_transform_normal(matrix: &[[f32;3];3], normal: [f32;3]) -> [f32;3] {
    core::array::from_fn(|r| (0..3).map(|c|matrix[c][r]*normal[c]).sum())
}

fn gl_lit_color(c: &WglContext, eye: [f32;4], normal: [f32;3], color: [f32;4]) -> Result<[f32;4], &'static str> {
    let fixed=&c.fixed;
    if !fixed.is_enabled(0x0b50) { return Ok(color.map(|v|v.clamp(0.0,1.0))); }
    if fixed.light_model_two_side { return Err("two-sided lighting needs back-face vertex colors"); }
    let mut material=fixed.materials[0].clone();
    if fixed.is_enabled(0x0b57) && matches!(fixed.color_material_face,0x0404|0x0408) {
        match fixed.color_material_mode {
            0x1200=>material.ambient=color,
            0x1201=>material.diffuse=color,
            0x1202=>material.specular=color,
            0x1600=>material.emission=color,
            0x1602=>{material.ambient=color;material.diffuse=color;},
            _=>return Err("unsupported color material mode"),
        }
    }
    if eye[3]==0.0 { return Err("lighting vertex at infinity"); }
    let eye_position=[eye[0]/eye[3],eye[1]/eye[3],eye[2]/eye[3]];
    let mut result=core::array::from_fn::<_,4,_>(|i|material.emission[i]+material.ambient[i]*c.light_model_ambient[i]);
    let viewer=if fixed.light_model_local_viewer { gl_vec_normalize(eye_position.map(|v|-v)) } else { [0.0,0.0,1.0] };
    for (index,light) in fixed.lights.iter().enumerate() {
        if !fixed.is_enabled(0x4000 + index as u32) { continue; }
        let p=light.position_eye;
        let delta=if p[3]==0.0 {[p[0],p[1],p[2]]} else {core::array::from_fn(|i|p[i]/p[3]-eye_position[i])};
        let distance=gl_vec_dot(delta,delta).sqrt();
        let direction=gl_vec_normalize(delta);
        let attenuation=if p[3]==0.0 {1.0} else {
            let a=light.attenuation;
            let denominator=a[0]+a[1]*distance+a[2]*distance*distance;
            if denominator>0.0 {1.0/denominator} else {return Err("zero light attenuation denominator");}
        };
        let spot=if light.spot_cutoff==180.0 {1.0} else {
            let d=gl_vec_dot(direction.map(|v|-v),gl_vec_normalize(light.spot_direction_eye)).max(0.0);
            if d < light.spot_cutoff.to_radians().cos() {0.0} else {d.powf(light.spot_exponent)}
        };
        let diffuse=gl_vec_dot(normal,direction).max(0.0);
        let half=gl_vec_normalize(core::array::from_fn(|i|direction[i]+viewer[i]));
        let specular=if diffuse>0.0 {gl_vec_dot(normal,half).max(0.0).powf(material.shininess)} else {0.0};
        for i in 0..3 {
            result[i]+=attenuation*spot*(material.ambient[i]*light.ambient[i]
                +diffuse*material.diffuse[i]*light.diffuse[i]
                +specular*material.specular[i]*light.specular[i]);
        }
    }
    result[3]=material.diffuse[3];
    if !result.iter().all(|v|v.is_finite()) { return Err("nonfinite lighting result"); }
    Ok(result.map(|v|v.clamp(0.0,1.0)))
}

#[cfg(test)]
mod staticgl_vertex_tests {
    use super::*;
    #[test]
    fn inverse_transpose_normal_transform_handles_nonuniform_scale() {
        let mut m=GL_IDENTITY_MATRIX;m[0]=2.0;m[5]=4.0;m[10]=8.0;
        assert_eq!(gl_transform_normal(&gl_normal_matrix(&m).unwrap(),[1.0;3]),[0.5,0.25,0.125]);
        m[5]=0.0;assert!(gl_normal_matrix(&m).is_err());
    }
}
