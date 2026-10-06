#version 440 core

#include <constants.glsl>

#define LIGHTING_TYPE LIGHTING_TYPE_REFLECTION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_GLOSSY

#if (FLUID_MODE == FLUID_MODE_LOW)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE
#elif (FLUID_MODE >= FLUID_MODE_MEDIUM)
#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_RADIANCE
#endif

// #define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_VOXEL
#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#define HAS_LOD_FULL_INFO

#include <globals.glsl>
#include <cloud.glsl>
#include <lod.glsl>

layout(location = 0) in vec3 f_pos;
layout(location = 1) in vec3 f_norm;
layout(location = 2) in float pull_down;

layout(location = 0) out vec4 tgt_color;
layout(location = 1) out uvec4 tgt_mat;

#include <sky.glsl>

void main() {
#ifdef EXPERIMENTAL_BAREMINIMUM
    tgt_color = vec4(simple_lighting(f_pos.xyz, lod_col(f_pos.xy), 1.0), 1);
    tgt_mat = uvec4(uvec3((f_norm + 1.0) * 127.0), MAT_LOD);
#else

    float my_alt = alt_at_real(f_pos.xy);
    vec3 f_pos = vec3(f_pos.xy, my_alt);
    vec3 cam_to_frag = normalize(f_pos - cam_pos.xyz);
    vec3 view_dir = -cam_to_frag;
    
    vec3 voxel_pos;
    vec3 voxel_norm;
    float voxel_sz;
    float f_ao;
    lod_voxels(f_pos, f_norm, cam_to_frag, voxel_pos, voxel_norm, voxel_sz, f_ao);
    
    vec3 f_col_raw = mix(lod_col(f_pos.xy), vec3(0), clamp(pull_down / 30, 0, 1));

    float shadow_alt = my_alt;

#if (SHADOW_MODE == SHADOW_MODE_CHEAP || SHADOW_MODE == SHADOW_MODE_MAP)
    vec4 f_shadow = textureMaybeBicubic(t_horizon, s_horizon, pos_to_tex(f_pos.xy));
    float sun_shade_frac = horizon_at2(f_shadow, shadow_alt, f_pos, sun_dir);
#elif (SHADOW_MODE == SHADOW_MODE_NONE)
    float sun_shade_frac = 1.0;
#endif
    float moon_shade_frac = 1.0;

    vec3 f_col = f_col_raw;
    
    DirectionalLight sun_info = get_sun_info(sun_dir, sun_shade_frac, /*sun_pos*/f_pos);
    DirectionalLight moon_info = get_moon_info(moon_dir, moon_shade_frac/*, light_pos*/);

    float alpha = 1.0;
    const float n2 = 1.5;
    const float R_s2s0 = pow(abs((1.0 - n2) / (1.0 + n2)), 2);
    const float R_s1s0 = pow(abs((1.3325 - n2) / (1.3325 + n2)), 2);
    const float R_s2s1 = pow(abs((1.0 - 1.3325) / (1.0 + 1.3325)), 2);
    const float R_s1s2 = pow(abs((1.3325 - 1.0) / (1.3325 + 1.0)), 2);
    float cam_alt = alt_at(cam_pos.xy);
    float fluid_alt = medium.x == MEDIUM_WATER ? max(cam_alt + 1, floor(shadow_alt)) : view_distance.w;
    float R_s = (f_pos.z < my_alt) ? mix(R_s2s1 * R_s1s0, R_s1s0, medium.x) : mix(R_s2s0, R_s1s2 * R_s2s0, medium.x);

    vec3 emitted_light, reflected_light;

    vec3 mu = medium.x == MEDIUM_WATER ? MU_WATER : vec3(0.0);
    // NOTE: Default intersection point is camera position, meaning if we fail to intersect we assume the whole camera is in water.
    vec3 cam_attenuation = compute_attenuation_point(f_pos, view_dir, mu, fluid_alt, cam_pos.xyz);

    float max_light = 0.0;
    vec3 k_a = vec3(1.0);
    vec3 k_d = vec3(1.0);
    max_light += get_sun_diffuse2(sun_info, moon_info, voxel_norm, view_dir, f_pos, vec3(0.0), cam_attenuation, fluid_alt, k_a, k_d, vec3(R_s), alpha, voxel_norm, 0.0, emitted_light, reflected_light);
    
    float ao = f_ao;
    emitted_light *= ao;
    reflected_light *= ao;

    vec3 surf_color;
    float surf_alpha = 1.0;
    uint mat;
    // NOTE: On nvidea vulkan drivers a `pow` with negative base results in NaN even if the exponent is an integer.
    vec3 water_col_diff = f_col_raw - vec3(0.02, 0.06, 0.22);
    if (dot(water_col_diff * water_col_diff, vec3(1)) < 0.01 && dot(vec3(0, 0, 1), f_norm) > 0.9) {
        mat = MAT_WATER;
        vec3 reflect_ray = cam_to_frag * vec3(1, 1, -1);
        #if (FLUID_MODE >= FLUID_MODE_MEDIUM)
            vec3 water_color = (1.0 - MU_WATER) * MU_SCATTER;

            float passthrough = dot(faceforward(f_norm, f_norm, cam_to_frag), -cam_to_frag);

            vec3 reflect_color;
            #if (FLUID_MODE == FLUID_MODE_HIGH)
                reflect_color = get_sky_color(reflect_ray, f_pos, vec3(-100000), 0.125, false, 1.0, true, sun_shade_frac);
                reflect_color = get_cloud_color(reflect_color, reflect_ray, cam_pos.xyz, 100000.0, 0.1);
            #else
                reflect_color = get_sky_color(reflect_ray, f_pos, vec3(-100000), 0.125, false, 1.0, true, sun_shade_frac);
            #endif
            reflect_color *= sun_shade_frac * 0.75 + 0.25;

            const float REFLECTANCE = 1.0;
            surf_color = illuminate(max_light, view_dir, f_col * emitted_light, reflect_color * REFLECTANCE + water_color * reflected_light);

            const vec3 underwater_col = vec3(0.0);
            float min_refl = min(emitted_light.r, min(emitted_light.g, emitted_light.b));
            surf_color = mix(underwater_col, surf_color, (1.0 - passthrough) * 1.0 / (1.0 + min_refl));
            surf_alpha = 1.0 - passthrough;
        #else
            surf_alpha = 0.9;
            surf_color = get_sky_color(reflect_ray, f_pos, vec3(-100000), 0.125, true, 1.0, true, sun_shade_frac);
        #endif
    } else {
        mat = MAT_LOD;
        surf_color = illuminate(max_light, view_dir, f_col * emitted_light, f_col * reflected_light);
    }

    tgt_color = vec4(surf_color, surf_alpha);
    tgt_mat = uvec4(uvec3((f_norm + 1.0) * 127.0), mat);
#endif
}
