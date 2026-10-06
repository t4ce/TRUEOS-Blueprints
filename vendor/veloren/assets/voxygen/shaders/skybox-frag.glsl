#version 440 core

#include <constants.glsl>

#define LIGHTING_TYPE LIGHTING_TYPE_TRANSMISSION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_SPECULAR

#if (FLUID_MODE == FLUID_MODE_LOW)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE
#elif (FLUID_MODE >= FLUID_MODE_MEDIUM)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_RADIANCE
#endif

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#include <globals.glsl>
#include <sky.glsl>
#include <lod.glsl>

layout(location = 0) in vec3 f_pos;

layout(location = 0) out vec4 tgt_color;
layout(location = 1) out uvec4 tgt_mat;

void main() {
#ifdef EXPERIMENTAL_BAREMINIMUM
    tgt_color = vec4(get_sky_color(), 1);
#else
    vec3 cam_dir = normalize(f_pos - cam_pos.xyz);

    float cam_alt = alt_at(cam_pos.xy);
    float fluid_alt = medium.x == MEDIUM_WATER ? floor(cam_alt + 1) : view_distance.w;
    vec3 mu = medium.x == MEDIUM_WATER ? MU_WATER : vec3(0.0);


    float fog_level = fog(f_pos.xyz, focus_pos.xyz, medium.x);

    float dist = 100000.0;

    float refractionIndex = medium.x == MEDIUM_WATER ? 1.0 / 1.3325 : 1.0;
    vec3 wpos = cam_pos.xyz + cam_dir * dist;

    tgt_color = vec4(get_sky_color(normalize(f_pos), cam_pos.xyz, wpos, 1.0, true, refractionIndex, false, 1.0), 1.0);
    tgt_mat = uvec4(uvec3(0), MAT_SKY);
#endif
}
