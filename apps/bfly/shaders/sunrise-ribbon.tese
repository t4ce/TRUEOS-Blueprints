#version 450 core

layout(quads, equal_spacing, ccw) in;
layout(location = 0) patch in int sweep_and_angle;
layout(location = 1) patch in int tag_and_phase;
layout(location = 2) patch in vec3 clip_from_x;
layout(location = 3) patch in vec3 clip_from_y;
layout(location = 4) patch in vec3 clip_from_origin;
layout(location = 5) patch in vec3 clip_from_mid_x;
layout(location = 6) patch in vec3 clip_from_tail_x;
// Backward flight offset in NDC and its share of the flap sweep.
layout(location = 7) patch in vec3 flight_wake;
layout(location = 0) out vec4 ribbon;
layout(location = 1) out vec4 tear_and_pan;
layout(location = 2) flat out uint wake_weights;

void main() {
    float t = gl_TessCoord.x;
    float age = gl_TessCoord.y;
    vec2 a = mix(gl_in[0].gl_Position.xy, gl_in[1].gl_Position.xy, t);
    vec2 b = mix(gl_in[1].gl_Position.xy, gl_in[2].gl_Position.xy, t);
    vec2 c = mix(gl_in[2].gl_Position.xy, gl_in[3].gl_Position.xy, t);
    vec2 point = mix(mix(a, b, t), mix(b, c, t), t);

    vec3 x_basis = age < 0.5 ? mix(clip_from_x, clip_from_mid_x, 2.0 * age)
                             : mix(clip_from_mid_x, clip_from_tail_x, 2.0 * age - 1.0);
    vec3 hinge = clip_from_y * point.y + clip_from_origin;
    vec3 clip = x_basis * point.x + hinge;
    vec3 head = clip_from_x * point.x + hinge;
    // Mix the flap and flight paths in projected space so perspective keeps
    // the share bounded. At age zero both positions coincide with the outline.
    clip.xy = mix(clip.xy, head.xy * (clip.z / head.z), flight_wake.z)
              + age * flight_wake.xy * clip.z;
    gl_Position = vec4(clip.xy, 0.5 * clip.z, clip.z);

    vec2 sweep = unpackHalf2x16(uint(sweep_and_angle));
    vec2 uv = vec2(0.87 * point.x + 0.67 * point.y, 0.44 * point.y + 0.29 * age);
    ribbon = vec4(uv, age, sweep.x);

    uint appearance = uint(tag_and_phase);
    wake_weights = appearance >> 16u;
    uint tag = (appearance >> 11u) & 31u;
    float phase = float(appearance & 2047u) * (6.2831853 / 2047.0);
    float drift = sin(phase - 1.6 * age);
    float curve_count = (tag & 2u) != 0u ? 5.0 : 3.0;
    float outline_position = (float((tag >> 2u) & 7u) + t) / curve_count;
    float side_offset = (tag & 1u) != 0u ? 0.10 : 0.21;
    vec2 tear_uv = vec2(0.17 + 0.66 * age,
                      0.76 * outline_position + side_offset + 0.04 * age);
    tear_and_pan = vec4(tear_uv, 0.18 * sweep.y, drift);
}
