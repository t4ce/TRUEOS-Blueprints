#version 450 core

layout(std140, binding = 0) uniform WingDisplay {
    mat4 clip_from_view;
    mat4 view_from_wing;
    vec4 side_bounds;
    vec4 framebuffer;
};

layout(location = 0) out vec2 mask_texel;
layout(location = 1) flat out int mask_layer;
layout(location = 2) flat out vec4 panel_pixels;
layout(location = 3) flat out int panel_background;

const int PANEL_COUNT = 8;

vec4 panel_rect(int index) {
    vec4 rest = vec4(0.0, 0.0, framebuffer.xy);
    for (int split_index = 0; split_index <= index && split_index < PANEL_COUNT - 1; ++split_index) {
        vec4 panel = rest;
        vec2 size = rest.zw - rest.xy;
        if (size.x >= size.y) {
            panel.z = rest.x + floor(size.x * 0.5);
            rest.x = panel.z;
        } else {
            panel.y = rest.y + floor(size.y * 0.5);
            rest.w = panel.y;
        }
        if (split_index == index) {
            return panel;
        }
    }
    return rest;
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

vec4 project_wing(vec2 point, float side, vec2 hinge, mat3 pose, float depth) {
    vec3 bent = vec3(side * point.x * hinge.x, point.y, point.x * hinge.y);
    vec4 view = view_from_wing * vec4(pose * bent, 1.0);
    view.z -= depth;
    return clip_from_view * view;
}

vec2 hinge_rotation(float phase) {
    float angle = radians(40.0) * sin(6.2831853 * phase);
    return vec2(cos(angle), sin(angle));
}

// Return the common mask tier for both sides of this butterfly.
int projected_layer(vec2 right_hinge, vec2 left_hinge, mat3 pose, float depth) {
    float tip_x = side_bounds.z;
    float bottom = side_bounds.y;
    float top = side_bounds.w;
    vec4 corners[6] = vec4[6](
        project_wing(vec2(0.0, bottom),   1.0, right_hinge, pose, depth),
        project_wing(vec2(0.0, top),      1.0, right_hinge, pose, depth),
        project_wing(vec2(tip_x, bottom), 1.0, right_hinge, pose, depth),
        project_wing(vec2(tip_x, top),    1.0, right_hinge, pose, depth),
        project_wing(vec2(tip_x, bottom), -1.0, left_hinge, pose, depth),
        project_wing(vec2(tip_x, top),    -1.0, left_hinge, pose, depth)
    );
    vec2 lo = vec2(3.402823e38);
    vec2 hi = -lo;
    for (int i = 0; i < 6; ++i) {
        if (corners[i].w <= 0.0) {
            // A rectangle may cross the eye plane. Leave clipping to rasterization.
            return 7;
        }
        vec2 ndc = corners[i].xy / corners[i].w;
        lo = min(lo, ndc);
        hi = max(hi, ndc);
    }

    vec2 extent = (hi - lo) * (0.5 * framebuffer.xy);
    if (any(lessThan(extent, vec2(4.0)))) {
        return -1;
    }
    uint span = uint(clamp(max(extent.x, extent.y), 8.0, 1024.0));
    return findMSB(span) - 3;
}

void main() {
#ifdef VULKAN
    int instance = gl_InstanceIndex;
    int corner = gl_VertexIndex;
#else
    int instance = gl_InstanceID;
    int corner = gl_VertexID;
#endif
    bool is_background = instance < PANEL_COUNT;
    int wing_instance = instance - PANEL_COUNT;
    int index = is_background ? instance : wing_instance >> 1;
    panel_background = int(is_background);
    panel_pixels = panel_rect(index);
    vec2 uv = vec2(float(corner & 1), float(corner >> 1));

    float long_side = max(framebuffer.x, framebuffer.y);
    float short_side = min(framebuffer.x, framebuffer.y);
    float frame_fit = min(1.0, min(long_side / 3072.0, short_side / 1728.0));
    float depth = framebuffer.w * float(1 << index) / frame_fit;
    mat3 pose = panel_rotation(index);
    float phase = fract(0.5 * framebuffer.z + float(index) / float(PANEL_COUNT));
    vec2 right_hinge = hinge_rotation(phase);
    vec2 left_hinge = hinge_rotation(phase + 0.125);

    mask_layer = projected_layer(right_hinge, left_hinge, pose, depth);
    mask_texel = vec2(0.0);
    if (is_background) {
        vec2 pixel = mix(panel_pixels.xy, panel_pixels.zw, uv);
        gl_Position = vec4(2.0 * pixel / framebuffer.xy - 1.0, 0.0, 1.0);
        return;
    }
    if (mask_layer < 0) {
        gl_Position = vec4(2.0, 2.0, 0.0, 1.0);
        return;
    }

    float side = ((wing_instance & 1) == 0) ? 1.0 : -1.0;
    vec2 point = mix(side_bounds.xy, side_bounds.zw, uv);
    vec2 hinge = (side > 0.0) ? right_hinge : left_hinge;
    gl_Position = project_wing(point, side, hinge, pose, depth);
    vec2 center_ndc = (panel_pixels.xy + panel_pixels.zw) / framebuffer.xy - 1.0;
    gl_Position.xy += center_ndc * gl_Position.w;

    // Each layer holds a padded N+1 texel square in its lower-left corner.
    float content_texels = float((8 << mask_layer) + 1);
    mask_texel = vec2(1.5) + uv * content_texels;
}
