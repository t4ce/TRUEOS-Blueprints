#version 450 core

#ifndef BFLY_NEUTRAL_OUTLINE
layout(location = 0) flat in uint segment_color;
#endif
layout(location = 0) out vec4 color;

#ifdef BFLY_NEUTRAL_OUTLINE
void main() {
    color = vec4(1.0);
}
#else
void main() {
    uint palette = segment_color & 3u;
    uint trail = segment_color >> 2;
    uvec3 primary = 255u * uvec3(equal(uvec3(palette), uvec3(0u, 1u, 2u)));
    // The history pass reads the strip and downstroke from the lowest RGB
    // bits. The live outline stays opaque with at most three byte levels of tint.
    uvec3 tag = uvec3(trail & 3u, (trail >> 2) & 1u, (trail >> 3) & 1u);
    color = vec4(vec3(primary ^ tag) * (1.0 / 255.0), 1.0);
}
#endif
