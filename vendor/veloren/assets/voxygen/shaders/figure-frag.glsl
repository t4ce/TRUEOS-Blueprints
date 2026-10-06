#version 440 core

#define FIGURE_SHADER

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

#define HAS_SHADOW_MAPS

#include <globals.glsl>
#include <light.glsl>
#include <cloud.glsl>
#include <lod.glsl>

#ifdef EXPERIMENTAL_DISCARDTRANSPARENCY
#include <random.glsl>
#endif

layout(location = 0) in vec3 f_pos;
layout(location = 1) flat in vec3 f_norm;
layout(location = 2) in vec2 f_uv_pos;
layout(location = 3) in vec3 m_pos;
layout(location = 4) in float scale;

layout(set = 2, binding = 0)
uniform texture2D t_col_light;
layout(set = 2, binding = 1)
uniform sampler s_col_light;

layout (std140, set = 3, binding = 0)
uniform u_locals {
    mat4 model_mat;
    vec4 highlight_col;
    vec4 model_light;
    vec4 model_glow;
    ivec4 atlas_offs;
    vec3 model_pos;
    // bit 0 - is player
    // bit 1-31 - unused
    int flags;
};

struct BoneData {
    mat4 bone_mat;
    mat4 normals_mat;
};

layout (std140, set = 3, binding = 1)
uniform u_bones {
    BoneData bones[16];
};

layout(location = 0) out vec4 tgt_color;
layout(location = 1) out uvec4 tgt_mat;

void main() {
    #ifdef EXPERIMENTAL_DISCARDTRANSPARENCY
    if ((flags & 1) == 1) {
        if (int(cam_mode) == 1) {
            float distance = distance(vec3(cam_pos), focus_pos.xyz) - 2;

            float opacity = clamp(distance / distance_divider, 0.5, 1);

            if (dither(gl_FragCoord.xy, opacity, 123456789)) {
                discard;
            }
        }

        if (int(cam_mode) == 0) {
            float s = min(screen_res.x, screen_res.y);
            float opacity = clamp(distance(gl_FragCoord.xy, screen_res.xy * 0.5) / s, 0.5, 1.0);

            if (dither(gl_FragCoord.xy, opacity, 123456789)) {
                discard;
            }
        }
    }
    #endif

    float f_ao;
    uint material = 0xFFu;
    vec3 f_col = greedy_extract_col_light_figure(t_col_light, s_col_light, f_uv_pos, f_ao, material);
    
#ifdef EXPERIMENTAL_BAREMINIMUM
    tgt_color = vec4(simple_lighting(f_pos.xyz, f_col, f_ao), 1);
#else

    vec3 cam_to_frag = normalize(f_pos - cam_pos.xyz);
    vec3 view_dir = -cam_to_frag;

#if (SHADOW_MODE == SHADOW_MODE_CHEAP || SHADOW_MODE == SHADOW_MODE_MAP || FLUID_MODE >= FLUID_MODE_MEDIUM)
    float f_alt = alt_at(f_pos.xy);
#elif (SHADOW_MODE == SHADOW_MODE_NONE || FLUID_MODE == FLUID_MODE_LOW)
    float f_alt = f_pos.z;
#endif

#if (SHADOW_MODE == SHADOW_MODE_CHEAP || SHADOW_MODE == SHADOW_MODE_MAP)
    vec4 f_shadow = textureMaybeBicubic(t_horizon, s_horizon, pos_to_tex(f_pos.xy));
    float sun_shade_frac = horizon_at2(f_shadow, f_alt, f_pos, sun_dir);
#elif (SHADOW_MODE == SHADOW_MODE_NONE)
    float sun_shade_frac = 1.0;
#endif
    float moon_shade_frac = 1.0;

    DirectionalLight sun_info = get_sun_info(sun_dir, sun_shade_frac, /*sun_pos*/f_pos);
    DirectionalLight moon_info = get_moon_info(moon_dir, moon_shade_frac/*, light_pos*/);

    vec3 surf_color;
    // If the figure is large enough to be 'terrain-like', we apply a noise effect to it
    #ifndef EXPERIMENTAL_NONOISE
        if (scale >= 0.5) {
            // TODO: Fix this, it isn't cprrect to use `f_norm` here. Would need something like
            // `m_norm` which is a normal relative to the figure.
            float noise = hash(vec4(floor(m_pos * 3.0 - vec3(0.5, 0, 0) - f_norm * 0.1), 0));

            const float A = 0.055;
            const float W_INV = 1 / (1 + A);
            const float W_2 = W_INV * W_INV;
            const float NOISE_FACTOR = 0.015;
            vec3 noise_delta = (sqrt(f_col) * W_INV + noise * NOISE_FACTOR);
            surf_color = noise_delta * noise_delta * W_2;
        } else
    #endif
    {
        surf_color = f_col;
    }

    float alpha = 1.0;
    const float n2 = 1.5;


    // This is a silly hack. It's not true reflectance (see below for that), but gives the desired
    // effect without breaking the entire lighting model until we come up with a better way of doing
    // reflectivity that accounts for physical surroundings like the ground
    if ((material & (1u << 1u)) > 0u) {
        vec3 reflect_ray_dir = reflect(cam_to_frag, f_norm);
        surf_color *= dot(vec3(1.0) - abs(fract(reflect_ray_dir * 1.5) * 2.0 - 1.0) * 0.85, vec3(1));
        alpha = 0.1;
    }

    const float R_s2s0 = pow(abs((1.0 - n2) / (1.0 + n2)), 2);
    const float R_s1s0 = pow(abs((1.3325 - n2) / (1.3325 + n2)), 2);
    const float R_s2s1 = pow(abs((1.0 - 1.3325) / (1.0 + 1.3325)), 2);
    const float R_s1s2 = pow(abs((1.3325 - 1.0) / (1.3325 + 1.0)), 2);
    float R_s = (f_pos.z < f_alt) ? mix(R_s2s1 * R_s1s0, R_s1s0, medium.x) : mix(R_s2s0, R_s1s2 * R_s2s0, medium.x);

    vec3 k_a = vec3(1.0);
    vec3 k_d = vec3(1.0);
    vec3 k_s = vec3(R_s);

    vec3 emitted_light, reflected_light;

    // Make voxel shadows block the sun and moon
    sun_info.block *= model_light.x;
    moon_info.block *= model_light.x;

    float max_light = 0.0;

    vec3 cam_attenuation = vec3(1);
    float fluid_alt = max(f_pos.z + 1, floor(f_alt + 1));
    vec3 mu = medium.x == MEDIUM_WATER ? MU_WATER : vec3(0.0);
    #if (FLUID_MODE >= FLUID_MODE_MEDIUM)
        cam_attenuation =
            medium.x == MEDIUM_WATER ? compute_attenuation_point(cam_pos.xyz, view_dir, mu, fluid_alt, f_pos)
            : compute_attenuation_point(f_pos, -view_dir, mu, fluid_alt, cam_pos.xyz);
    #endif

    // Prevent the sky affecting light when underground
    float not_underground = clamp((f_pos.z - f_alt) / 128.0 + 1.0, 0.0, 1.0);

    max_light += get_sun_diffuse2(sun_info, moon_info, f_norm, view_dir, f_pos, mu, cam_attenuation, fluid_alt, k_a, k_d, k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);

    max_light += lights_at(f_pos, f_norm, view_dir, mu, cam_attenuation, fluid_alt, k_a, k_d, k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);

    // Apply baked lighting from emissive blocks
    float glow_mag = length(model_glow.xyz) + 0.001;
    vec3 glow = pow(model_glow.w, 3.0) * 6.0
        * glow_light(f_pos)
        * mix((max(dot(f_norm, model_glow.xyz / glow_mag) * 0.5 + 0.5, 0.0)), 1.0, 1.0 / (1.0 + glow_mag * 10.0));
    reflected_light += glow * cam_attenuation;

    // Apply baked AO
    float ao = f_ao * sqrt(f_ao);
    reflected_light *= ao;
    emitted_light *= ao;

    // Apply point light AO
    float point_shadow = shadow_at(f_pos, f_norm);
    reflected_light *= point_shadow;
    emitted_light *= point_shadow;
    
    float render_alpha = 1.0;
    uint render_mat = MAT_FIGURE;

    if ((material & 31u) != 0) {
        apply_cell_material(material, f_pos, f_norm, surf_color, emitted_light, render_alpha, render_mat);
    }

    float reflectance = 0.0;
    // TODO: Do reflectance properly like this later
    vec3 reflect_color = vec3(0);

    surf_color = illuminate(max_light, view_dir, mix(surf_color * emitted_light, reflect_color, reflectance), mix(surf_color * reflected_light, reflect_color, reflectance)) * highlight_col.rgb;

    tgt_color = vec4(surf_color, render_alpha);
    tgt_mat = uvec4(uvec3((f_norm + 1.0) * 127.0), render_mat);
#endif
}
