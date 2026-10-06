#version 440 core

#include <constants.glsl>

#define LIGHTING_TYPE (LIGHTING_TYPE_TRANSMISSION | LIGHTING_TYPE_REFLECTION)

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_SPECULAR

#if (FLUID_MODE == FLUID_MODE_LOW)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE
#elif (FLUID_MODE >= FLUID_MODE_MEDIUM)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_RADIANCE
#endif

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#include <globals.glsl>
#include <srgb.glsl>
#include <random.glsl>

layout(location = 0) in uint v_pos_norm;
layout(location = 1) in uint v_vel;

layout(std140, set = 2, binding = 0)
uniform u_locals {
    mat4 model_mat;
    ivec4 atlas_offs;
    float load_time;
};

layout(location = 0) out vec3 f_pos;
layout(location = 1) flat out uint f_pos_norm;
layout(location = 2) out vec2 f_vel;

const float EXTRA_NEG_Z = 65536.0;

void main() {
    vec3 rel_pos = vec3(v_pos_norm & 0x3Fu, (v_pos_norm >> 6) & 0x3Fu, float((v_pos_norm >> 12) & 0x1FFFFu) - EXTRA_NEG_Z);
    f_pos = (model_mat * vec4(rel_pos, 1.0)).xyz - focus_off.xyz;

    f_vel = vec2(
        (float(v_vel & 0xFFFFu) - 32768.0) / 1000.0,
        (float((v_vel >> 16u) & 0xFFFFu) - 32768.0) / 1000.0
    );

    // Terrain 'pop-in' effect
    #ifndef EXPERIMENTAL_BAREMINIMUM
        #ifdef EXPERIMENTAL_TERRAINPOP
            f_pos.z -= 250.0 * (1.0 - min(1.0001 - 0.02 / pow(time_since(load_time), 10.0), 1.0));
        #endif
    #endif

    #ifdef EXPERIMENTAL_CURVEDWORLD
        f_pos.z -= pow(distance(f_pos.xy + focus_off.xy, focus_pos.xy + focus_off.xy) * 0.05, 2);
    #endif

    f_pos_norm = v_pos_norm;

    gl_Position = all_mat * vec4(f_pos, 1);
}
