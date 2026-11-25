#version 450 core

#ifdef BFLY_BREED_FILL
layout(location = 0) flat in uint fill_rgb;
#endif
layout(location = 0) out vec4 color;

void main() {
#ifdef BFLY_BREED_FILL
    uvec3 rgb = (uvec3(fill_rgb) >> uvec3(0u, 8u, 16u)) & 255u;
#ifdef BFLY_TRANSLUCENT_FILL
    const float opacity = 0.85;
    color = vec4(vec3(rgb) * (opacity / 255.0), opacity);
#else
    color = vec4(vec3(rgb) * (1.0 / 255.0), 1.0);
#endif
#else
    color = vec4(vec3(128.0 / 255.0), 1.0);
#endif
}
