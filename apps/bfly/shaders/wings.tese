#version 450 core

layout(isolines, equal_spacing) in;
layout(location = 0) patch in int isoline_count;
layout(location = 1) patch in float straight_edge;

layout(std140, binding = 0) uniform WingView {
    mat4 clip_from_view;
    vec4 local_bounds;
    vec4 framebuffer_px;
    mat4 wing_orientation;
};

void main() {
    float lines = float(isoline_count);
    float interval = round(gl_TessCoord.y * lines);
    float t = (interval + gl_TessCoord.x) / lines;

    vec2 p0 = gl_in[0].gl_Position.xy;
    vec2 p3 = gl_in[3].gl_Position.xy;
    vec2 point = mix(p0, p3, t);
    if (straight_edge == 0.0) {
        vec2 p1 = gl_in[1].gl_Position.xy;
        vec2 p2 = gl_in[2].gl_Position.xy;
        vec2 a = mix(p0, p1, t);
        vec2 b = mix(p1, p2, t);
        vec2 c = mix(p2, p3, t);
        point = mix(mix(a, b, t), mix(b, c, t), t);
    }
    vec4 oriented = wing_orientation * vec4(point, 0.0, 1.0);
    oriented.z += gl_in[0].gl_Position.z;
    gl_Position = clip_from_view * oriented;
}
