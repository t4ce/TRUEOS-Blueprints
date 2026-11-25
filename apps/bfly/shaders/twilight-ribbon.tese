#version 450 core

layout(quads, equal_spacing, ccw) in;
layout(location = 0) patch in int ribbon_appearance;
layout(location = 1) patch in int ribbon_seed;
layout(location = 2) patch in vec3 clip_from_x;
layout(location = 3) patch in vec3 clip_from_y;
layout(location = 4) patch in vec3 clip_from_origin;
layout(location = 5) patch in vec3 clip_from_mid_x;
layout(location = 6) patch in vec3 clip_from_tail_x;
layout(location = 0) out vec4 ribbon;

void main() {
    float t = gl_TessCoord.x;
    float age = gl_TessCoord.y;
    vec2 a = mix(gl_in[0].gl_Position.xy, gl_in[1].gl_Position.xy, t);
    vec2 b = mix(gl_in[1].gl_Position.xy, gl_in[2].gl_Position.xy, t);
    vec2 c = mix(gl_in[2].gl_Position.xy, gl_in[3].gl_Position.xy, t);
    vec2 point = mix(mix(a, b, t), mix(b, c, t), t);

    vec3 x_basis = age < 0.5 ? mix(clip_from_x, clip_from_mid_x, 2.0 * age)
                             : mix(clip_from_mid_x, clip_from_tail_x, 2.0 * age - 1.0);
    vec3 clip = x_basis * point.x + clip_from_y * point.y + clip_from_origin;
    gl_Position = vec4(clip.xy, 0.5 * clip.z, clip.z);

    // Parameterize from the actual outline point to keep adjacent cubics seamless.
    vec2 uv = vec2(0.87 * point.x + 0.67 * point.y, 0.44 * point.y + 0.29 * age);
    uv += fract(float(ribbon_seed) * vec2(0.618034, 0.414214));
    float strength = float(ribbon_appearance & 255) / 255.0;
    float style = (ribbon_appearance & 256) != 0 ? 0.49 : 0.36;
    ribbon = vec4(uv, age, style * strength);
}
