#version 450 core

layout(binding = 1) uniform sampler2DArray wing_masks;
layout(location = 0) in vec2 mask_texel;
layout(location = 1) flat in int mask_layer;
layout(location = 2) flat in vec4 panel_pixels;
layout(location = 3) flat in int panel_background;
layout(location = 0) out vec4 color;

const uint DIGITS[10] = uint[10](
    0x7b6fu, 0x749au, 0x73e7u, 0x79e7u, 0x49edu,
    0x79cfu, 0x7bcfu, 0x4927u, 0x7befu, 0x79efu
);

// Three-by-five digit glyphs, packed from the upper left into the low bits.
bool label_pixel(vec2 panel_point, int segments) {
    int digits = (segments >= 1000) ? 4 : (segments >= 100) ? 3 : (segments >= 10) ? 2 : 1;
    vec2 label = (panel_point - 12.0) / 3.0;
    if (any(lessThan(label, vec2(0.0))) || label.x >= 4 * digits || label.y >= 5.0) {
        return false;
    }
    ivec2 cell = ivec2(label);
    int column = cell.x % 4;
    if (column == 3) {
        return false;
    }
    for (int i = 1 + cell.x / 4; i < digits; ++i) {
        segments /= 10;
    }
    return ((DIGITS[segments % 10] >> uint(cell.y * 3 + column)) & 1u) != 0u;
}

void main() {
    vec2 pixel = gl_FragCoord.xy;
    if (panel_background != 0) {
        vec2 edge = min(pixel - panel_pixels.xy, panel_pixels.zw - pixel);
        vec3 fill = (min(edge.x, edge.y) < 1.0) ? vec3(0.17, 0.19, 0.23) : vec3(0.025, 0.03, 0.04);
        vec2 upper_left = vec2(pixel.x - panel_pixels.x, panel_pixels.w - pixel.y);
        if (mask_layer >= 0 && label_pixel(upper_left, 8 << mask_layer)) {
            fill = vec3(0.52, 0.62, 0.72);
        }
        color = vec4(fill, 1.0);
        return;
    }
    if (any(lessThan(pixel, panel_pixels.xy)) || any(greaterThanEqual(pixel, panel_pixels.zw))) {
        discard;
    }
    vec2 uv = mask_texel / vec2(textureSize(wing_masks, 0).xy);
    float coverage = texture(wing_masks, vec3(uv, float(mask_layer))).r;
    color = vec4(coverage);
}
