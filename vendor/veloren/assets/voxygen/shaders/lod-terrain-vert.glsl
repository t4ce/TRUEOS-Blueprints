#version 440 core

#include <constants.glsl>

#define LIGHTING_TYPE LIGHTING_TYPE_REFLECTION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_GLOSSY

#if (FLUID_MODE == FLUID_MODE_LOW)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE
#elif (FLUID_MODE >= FLUID_MODE_MEDIUM)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_RADIANCE
#endif

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_VOXEL

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#include <globals.glsl>
#include <srgb.glsl>
#include <lod.glsl>

layout(location = 0) in vec2 v_pos;

layout(location = 0) out vec3 f_pos;
layout(location = 1) out vec3 f_norm;
layout(location = 2) out float pull_down;

void main() {
    // Find distances between vertices. Pull down a tiny bit more to reduce z fighting near the ocean.
    f_pos = lod_pos(v_pos, focus_pos.xy) - vec3(0, 0, 0.1);
    #ifndef EXPERIMENTAL_BAREMINIMUM
        vec2 dims = vec2(1.0 / view_distance.y);
        vec4 f_square = focus_pos.xyxy + vec4(splay(v_pos - dims), splay(v_pos + dims));
        f_norm = lod_norm(f_pos.xy, f_square);
    #endif
    
    pull_down = 1.0 / pow(distance(focus_pos.xy, f_pos.xy) / (view_distance.x * 0.95), 20.0);
    f_pos.z -= pull_down;

    #ifdef EXPERIMENTAL_CURVEDWORLD
        f_pos.z -= pow(distance(f_pos.xy + focus_off.xy, focus_pos.xy + focus_off.xy) * 0.05, 2);
    #endif

    gl_Position = all_mat * vec4(f_pos, 1);
}
