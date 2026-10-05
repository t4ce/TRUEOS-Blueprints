struct Camera {
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    projection: vec4<f32>, // focal length, aspect, near, far
};
@group(0) @binding(0) var<uniform> camera: Camera;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};
@vertex fn vs_main(@location(0) position: vec4<f32>, @location(1) color: vec4<f32>) -> Output {
    var out: Output;
    if position.w == 0.0 {
        out.position = vec4<f32>(position.xyz, 1.0);
    } else {
        let delta = position.xyz - camera.eye.xyz;
        let z = dot(delta, camera.forward.xyz);
        let near = camera.projection.z;
        let far = camera.projection.w;
        out.position = vec4<f32>(
            dot(delta, camera.right.xyz) * camera.projection.x / camera.projection.y,
            dot(delta, camera.up.xyz) * camera.projection.x,
            far / (far - near) * z - near * far / (far - near), z);
    }
    out.color = color;
    return out;
}
@fragment fn fs_main(in: Output) -> @location(0) vec4<f32> {
    return in.color;
}
