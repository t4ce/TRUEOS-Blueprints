#version 330 core
// #extension ARB_texture_storage : enable

#include <constants.glsl>

#define LIGHTING_TYPE LIGHTING_TYPE_REFLECTION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_GLOSSY

#if (FLUID_MODE == FLUID_MODE_LOW)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE
#elif (FLUID_MODE >= FLUID_MODE_MEDIUM)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_RADIANCE
#endif

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

// Currently, we only need globals for focus_off.
#include <globals.glsl>

/* Accurate packed shadow maps for many lights at once!
 *
 * Ideally, we would just write to a bitmask...
 *
 * */

layout(location = 1) in uint v_pos_norm;

// Light projection matrices.
layout (std140, set = 1,  binding = 0)
uniform u_locals {
    mat4 model_mat;
    ivec4 atlas_offs;
    float load_time;
};

const int EXTRA_NEG_Z = 32768;

void main() {
    vec3 f_chunk_pos = vec3(ivec3((uvec3(v_pos_norm) >> uvec3(0, 6, 12)) & uvec3(0x3Fu, 0x3Fu, 0xFFFFu)) - ivec3(0, 0, EXTRA_NEG_Z));
    vec3 f_pos = (model_mat * vec4(f_chunk_pos, 1.0)).xyz - focus_off.xyz;

    gl_Position = vec4(f_pos, 1.0);
}
