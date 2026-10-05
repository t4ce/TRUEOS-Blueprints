struct Camera {
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    projection: vec4<f32>, // focal length, aspect, near, far
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var atlas_texture: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) atlas_uv: vec4<f32>,
};
@vertex fn vs_main(@location(0) position: vec4<f32>, @location(1) atlas_uv: vec4<f32>) -> Output {
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
    out.atlas_uv = atlas_uv;
    return out;
}
@fragment fn fs_main(in: Output) -> @location(0) vec4<f32> {
    return textureSample(atlas_texture, atlas_sampler, in.atlas_uv.xy);
}
