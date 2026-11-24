#version 450 core

layout(isolines, equal_spacing) in;
layout(location = 0) patch in int isoline_count;
layout(location = 1) patch in int straight_edge;
layout(location = 2) patch in vec3 clip_from_x;
layout(location = 3) patch in vec3 clip_from_y;
layout(location = 4) patch in vec3 clip_from_origin;

void main() {
    float lines = float(isoline_count);
    float interval = round(gl_TessCoord.y * lines);
    float t = (interval + gl_TessCoord.x) / lines;

    vec2 p0 = gl_in[0].gl_Position.xy;
    vec2 p3 = gl_in[3].gl_Position.xy;
    vec2 point;
    if (straight_edge != 0) {
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
