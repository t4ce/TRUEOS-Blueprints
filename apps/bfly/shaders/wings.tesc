#version 450 core

layout(vertices = 4) out;
layout(location = 0) in float patch_straight[];
layout(location = 0) patch out int isoline_count;
layout(location = 1) patch out float straight_edge;

layout(std140, binding = 0) uniform WingView {
    mat4 clip_from_view;
    vec4 local_bounds;
    vec4 framebuffer_px;
    mat4 wing_orientation;
};

vec2 project(vec2 point, float view_z) {
    vec4 oriented = wing_orientation * vec4(point, 0.0, 1.0);
    oriented.z += view_z;
    vec4 clip = clip_from_view * oriented;
    return clip.xy / clip.w;
}

vec2 projected_extent(float view_z) {
    vec2 a = project(local_bounds.xy, view_z);
    vec2 b = project(local_bounds.xw, view_z);
    vec2 c = project(local_bounds.zy, view_z);
    vec2 d = project(local_bounds.zw, view_z);
    vec2 lo = min(min(a, b), min(c, d));
    vec2 hi = max(max(a, b), max(c, d));
    return (hi - lo) * (0.5 * framebuffer_px.xy);
}

void main() {
    gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position;
    if (gl_InvocationID != 0) {
        return;
    }

    straight_edge = patch_straight[0];
    vec2 extent = projected_extent(gl_in[0].gl_Position.z);
    if (any(lessThan(extent, vec2(4.0)))) {
        isoline_count = 0;
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

    uint pixel_span = uint(clamp(max(extent.x, extent.y), 8.0, 1024.0));
    int segments = (straight_edge != 0.0) ? 1 : 1 << findMSB(pixel_span);
    // Each of the isolines covers one disjoint interval of the same cubic.
    isoline_count = max(1, segments / 64);
    gl_TessLevelOuter[0] = float(isoline_count);
    gl_TessLevelOuter[1] = float(segments / isoline_count);
}
