#version 450 core

layout(vertices = 4) out;
layout(location = 0) flat in int panel_side[];
layout(location = 0) patch out int isoline_count;
layout(location = 1) patch out int straight_edge;
layout(location = 2) patch out vec3 clip_from_x;
layout(location = 3) patch out vec3 clip_from_y;
layout(location = 4) patch out vec3 clip_from_origin;

layout(std430, binding = 0) readonly buffer Camera {
    layout(offset = 288) vec4 framebuffer;
};

const int PANEL_COUNT = 8;
const float FOCAL_LENGTH = 8000.0;
const float TAU = 6.2831853;
const vec3 SIDE_BOUNDS = vec3(-0.33208677881903803, 0.6622233225082153, 0.3311099279079387);

vec4 panel_rect(int index, vec2 size) {
    vec4 rest = vec4(0.0, 0.0, size);
    for (int split = 0; split <= index && split < PANEL_COUNT - 1; ++split) {
        vec4 panel = rest;
        vec2 extent = rest.zw - rest.xy;
        if (extent.x >= extent.y) {
            panel.z = rest.x + floor(extent.x * 0.5);
            rest.x = panel.z;
        } else {
            panel.y = rest.y + floor(extent.y * 0.5);
            rest.w = panel.y;
        }
        if (split == index) {
            return panel;
        }
    }
    return rest;
}

vec3 flight_knot(int knot, int count, float angle_offset) {
    // Wrap the knot index before computing its angle so adjacent segments
    // share exactly the same endpoints and tangents across the lap seam.
    float angle = float((knot + count) % count) * (TAU / float(count)) + angle_offset;
    float x = cos(angle);
    float y = sin(angle);
    return vec3(x, 0.82 * y, x * (0.38 + 0.48 * y));
}

void flight_path(int index, float time, float lap_seconds, out vec3 position, out vec3 tangent) {
    int count = 3 + index % 5;
    float phase = fract(fract(time / lap_seconds) + float(index) / float(PANEL_COUNT));
    float progress = phase * float(count);
    int segment = int(progress);
    float t = fract(progress);
    float angle_offset = 0.6 * float(index);

    // Closed Catmull–Rom spline with a derivative in view-space direction.
    vec3 a = flight_knot(segment - 1, count, angle_offset);
    vec3 b = flight_knot(segment, count, angle_offset);
    vec3 c = flight_knot(segment + 1, count, angle_offset);
    vec3 d = flight_knot(segment + 2, count, angle_offset);
    vec3 start = 0.5 * (c - a);
    vec3 end = 0.5 * (d - b);
    vec3 span = c - b;
    vec3 quadratic = 3.0 * span - 2.0 * start - end;
    vec3 cubic = start + end - 2.0 * span;
    position = b + t * (start + t * (quadratic + t * cubic));
    tangent = start + t * (2.0 * quadratic + 3.0 * t * cubic);
}

mat3 flight_orientation(vec3 tangent) {
    // The closed paths circle in XY, keeping this reference cross product
    // well-defined even when the heading has a component in depth.
    vec3 forward = normalize(tangent);
    vec3 right = normalize(cross(forward, vec3(0.0, 0.0, 1.0)));
    return mat3(right, forward, cross(right, forward));
}

mat3 panel_rotation(int index) {
    float roll = radians(12.0 + 5.0 * float(index));
    float tilt = radians(8.0 + 2.5 * float(index));
    roll *= ((index & 1) == 0) ? 1.0 : -1.0;
    tilt *= ((index & 2) == 0) ? 1.0 : -1.0;
    float c = cos(roll);
    float s = sin(roll);
    float a = cos(tilt);
    float b = sin(tilt);
    return mat3(c, s * a, s * b, -s, c * a, c * b, 0.0, -b, a);
}

vec2 flap_angles(int index, float time) {
    float offset = float(index) / float(PANEL_COUNT);
    float relaxed_phase = 0.25 * time + offset;
    vec2 relaxed = radians(32.0) * sin(TAU * fract(vec2(relaxed_phase) + vec2(0.0, 0.135)));

    float beat = time / 1.25 + offset;
    // Five is coprime to sixteen, so each butterfly cycles through every sweep.
    int sweep_step = (int(beat) * 5 + index * 3) & 15;
    float sweep = 75.0 + 5.0 * float(sweep_step);
    // Pick the sweep at the common upper turnaround, where the velocity is zero.
    float downstroke = 0.5 - 0.5 * cos(TAU * fract(beat));
    float synchronized = radians(87.75 - sweep * downstroke);

    float blend = smoothstep(30.0, 30.75, time) * (1.0 - smoothstep(59.25, 60.0, time));
    return mix(relaxed, vec2(synchronized), blend);
}

vec2 project(vec3 view, float depth) {
    return FOCAL_LENGTH * view.xy / (depth - view.z);
}

vec2 projected_extent(vec3 right, vec3 left, vec3 y_basis, vec3 position, float depth) {
    vec3 bottom = position + SIDE_BOUNDS.x * y_basis;
    vec3 top = position + SIDE_BOUNDS.z * y_basis;
    vec2 corners[6] = vec2[6](
        project(bottom, depth),
        project(top, depth),
        project(bottom + SIDE_BOUNDS.y * right, depth),
        project(top + SIDE_BOUNDS.y * right, depth),
        project(bottom + SIDE_BOUNDS.y * left, depth),
        project(top + SIDE_BOUNDS.y * left, depth)
    );
    vec2 lo = corners[0];
    vec2 hi = corners[0];
    for (int i = 1; i < 6; ++i) {
        lo = min(lo, corners[i]);
        hi = max(hi, corners[i]);
    }
    return hi - lo;
}

vec3 clip_basis(vec3 axis, vec2 scale, vec2 center) {
    return vec3(axis.xy * scale - center * axis.z, -axis.z);
}

void main() {
    gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position;
    if (gl_InvocationID != 0) {
        return;
    }

    int index = panel_side[0] >> 1;
    bool right_side = (panel_side[0] & 1) == 0;
    straight_edge = int(gl_in[0].gl_Position.z);
    // Both mirrored sides meet at the same straight connector.
    if (straight_edge != 0 && !right_side) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }
    vec4 frame = framebuffer;
    float time = mod(frame.z, 60.0);
    vec4 panel = panel_rect(index, frame.xy);
    vec2 panel_size = panel.zw - panel.xy;
    float fit = min(1.0, min(max(frame.x, frame.y) / 3072.0, min(frame.x, frame.y) / 1728.0));
    float depth = 6.0 * float(1 << index) / fit;
    vec3 position = vec3(0.0);
    mat3 pose;
    if (frame.w > 0.0) {
        vec3 tangent;
        flight_path(index, time, frame.w, position, tangent);
        float radius = 0.16 * min(panel_size.x, panel_size.y) * depth / FOCAL_LENGTH;
        position *= radius;
        pose = flight_orientation(tangent);
    } else {
        pose = panel_rotation(index);
    }
    vec2 angles = flap_angles(index, time);
    vec2 across = cos(angles);
    vec2 depth_offset = sin(angles);
    vec3 right = pose * vec3(across.x, 0.0, depth_offset.x);
    vec3 left = pose * vec3(-across.y, 0.0, depth_offset.y);
    vec3 y_basis = pose[1];
    vec2 extent = projected_extent(right, left, y_basis, position, depth);
    if (any(lessThan(extent, vec2(4.0)))) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

    uint span = uint(clamp(max(extent.x, extent.y), 8.0, 1024.0));
    int segments = (straight_edge != 0) ? 1 : 1 << findMSB(span);
    // Every isoline covers one interval of the same cubic.
    isoline_count = max(1, segments / 64);
    gl_TessLevelOuter[0] = float(isoline_count);
    gl_TessLevelOuter[1] = float(segments / isoline_count);

    vec2 scale = (2.0 * FOCAL_LENGTH) / frame.xy;
    vec2 center = (panel.xy + panel.zw) / frame.xy - 1.0;
    vec3 x_basis = right_side ? right : left;
    clip_from_x = clip_basis(x_basis, scale, center);
    clip_from_y = clip_basis(y_basis, scale, center);
    clip_from_origin = clip_basis(position, scale, center) + vec3(center, 1.0) * depth;
}
