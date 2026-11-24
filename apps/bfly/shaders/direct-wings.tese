#version 450 core

layout(isolines, equal_spacing) in;
layout(location = 0) patch in int isoline_count;
layout(location = 1) patch in int straight_edge;
layout(location = 2) patch in vec3 clip_from_x;
layout(location = 3) patch in vec3 clip_from_y;
layout(location = 4) patch in vec3 clip_from_origin;
layout(location = 0) flat out uint segment_color;

void main() {
    float lines = float(isoline_count);
    float interval = round(gl_TessCoord.y * lines);
    float t = (interval + gl_TessCoord.x) / lines;
    // Count across isolines so the palette continues through each cubic.
    // The provoking vertex supplies a single color index for the entire line.
    float segments_per_line = gl_TessLevelOuter[1];
    uint segment = uint(interval * segments_per_line + round(gl_TessCoord.x * segments_per_line));
    segment_color = (segment % 3u) | (uint(straight_edge) & ~3u);

    vec2 p0 = gl_in[0].gl_Position.xy;
    vec2 p3 = gl_in[3].gl_Position.xy;
    vec2 point;
    int edge_kind = straight_edge & 3;
    if (edge_kind == 2) {
        point = vec2(0.0, t);
    } else if (edge_kind != 0) {
        point = mix(p0, p3, t);
    } else {
        vec2 p1 = gl_in[1].gl_Position.xy;
        vec2 p2 = gl_in[2].gl_Position.xy;
        vec2 a = mix(p0, p1, t);
        vec2 b = mix(p1, p2, t);
        vec2 c = mix(p2, p3, t);
        point = mix(mix(a, b, t), mix(b, c, t), t);
    }

    vec3 clip = clip_from_x * point.x + clip_from_y * point.y + clip_from_origin;
    gl_Position = vec4(clip.xy, 0.5 * clip.z, clip.z);
}
