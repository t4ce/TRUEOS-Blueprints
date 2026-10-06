#version 440 core
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

layout(location = 0) in uint v_pos_norm;

// Light projection matrices.
layout (std140, set = 1,  binding = 0)
uniform u_locals {
    mat4 model_mat;
    ivec4 atlas_offs;
    float load_time;
};

const float EXTRA_NEG_Z = 32768.0;

layout( push_constant ) uniform PointLightMatrix {
  mat4 lightShadowMatrix;
};

void main() {
    vec3 f_chunk_pos = vec3(v_pos_norm & 0x3Fu, (v_pos_norm >> 6) & 0x3Fu, float((v_pos_norm >> 12) & 0xFFFFu) - EXTRA_NEG_Z);
    vec3 f_pos = (model_mat * vec4(f_chunk_pos, 1.0)).xyz - focus_off.xyz;
    
    gl_Position = lightShadowMatrix * vec4(f_pos, 1.0);
}
