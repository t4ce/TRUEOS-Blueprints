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

#define HAS_SHADOW_MAPS

#include <globals.glsl>
#include <random.glsl>

layout(location = 0) in vec3 f_pos;
layout(location = 1) flat in uint f_pos_norm;
layout(location = 2) in vec2 f_vel;

layout(std140, set = 2, binding = 0)
uniform u_locals {
    mat4 model_mat;
    ivec4 atlas_offs;
    float load_time;
};

layout(location = 0) out vec4 tgt_color;
layout(location = 1) out uvec4 tgt_mat;

#include <sky.glsl>
#include <light.glsl>
#include <lod.glsl>

vec4 water_col(vec4 posx, vec4 posy) {
    posx = (posx + focus_off.x) * 0.1;
    posy = (posy + focus_off.y) * 0.1;
    return 0.5 + (vec4(
        textureLod(sampler2D(t_noise, s_noise), vec2(posx.x, posy.x), 0).x,
        textureLod(sampler2D(t_noise, s_noise), vec2(posx.y, posy.y), 0).x,
        textureLod(sampler2D(t_noise, s_noise), vec2(posx.z, posy.z), 0).x,
        textureLod(sampler2D(t_noise, s_noise), vec2(posx.w, posy.w), 0).x
    ) - 0.5) * 1.0;
}

float water_col_vel(vec2 pos){
    vec4 cols = water_col(
        pos.x - tick.z * floor(f_vel.x) - vec2(0.0, tick.z).xyxy,
        pos.y - tick.z * floor(f_vel.y) - vec2(0.0, tick.z).xxyy
    );
    return mix(
        mix(cols.x, cols.y, fract(f_vel.x + 1.0)),
        mix(cols.z, cols.w, fract(f_vel.x + 1.0)),
        fract(f_vel.y + 1.0)
    );
}

void main() {
#ifdef EXPERIMENTAL_BAREMINIMUM
    tgt_color = vec4(simple_lighting(f_pos.xyz, MU_SCATTER, 1.0), 0.5);
#else

    // First 3 normals are negative, next 3 are positive
    vec3 normals[6] = vec3[](vec3(-1,0,0), vec3(1,0,0), vec3(0,-1,0), vec3(0,1,0), vec3(0,0,-1), vec3(0,0,1));

    // TODO: last 3 bits in v_pos_norm should be a number between 0 and 5, rather than 0-2 and a direction.
    uint norm_axis = (f_pos_norm >> 30) & 0x3u;
    // Increase array access by 3 to access positive values
    uint norm_dir = ((f_pos_norm >> 29) & 0x1u) * 3u;
    // Use an array to avoid conditional branching
    vec3 f_norm = normals[norm_axis + norm_dir];

    vec3 cam_to_frag = normalize(f_pos - cam_pos.xyz);
    vec3 view_dir = -cam_to_frag;

    float water_shade = water_col_vel(f_pos.xy);
    vec3 water_color = (1.0 - mix(MU_WATER, pow(vec3(0.8, 0.9, 0.08), vec3(0.25)), water_shade)) * MU_SCATTER;

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

    float fluid_alt = f_pos.z;

    const float alpha = 0.255;
    const float n2 = 1.3325;
    const float R_s2s0 = pow(abs((1.0 - n2) / (1.0 + n2)), 2);
    const float R_s1s0 = pow(abs((1.3325 - n2) / (1.3325 + n2)), 2);
    const float R_s2s1 = pow(abs((1.0 - 1.3325) / (1.0 + 1.3325)), 2);
    const float R_s1s2 = pow(abs((1.3325 - 1.0) / (1.3325 + 1.0)), 2);
    float R_s = (f_pos.z < fluid_alt) ? mix(R_s2s1 * R_s1s0, R_s1s0, medium.x) : mix(R_s2s0, R_s1s2 * R_s2s0, medium.x);

    // Water is transparent so both normals are valid.
    vec3 cam_norm = faceforward(f_norm, f_norm, cam_to_frag);

    vec3 mu = MU_WATER;
    // NOTE: Default intersection point is camera position, meaning if we fail to intersect we assume the whole camera is in water.
    vec3 cam_attenuation = vec3(1.0);

    // NOTE: Assumes normal is vertical.
    vec3 sun_view_dir = cam_pos.z <= fluid_alt ? -view_dir : view_dir;

    vec3 k_a = vec3(1.0);
    vec3 k_d = vec3(1.0);
    vec3 k_s = vec3(R_s);

    vec3 reflect_ray_dir = reflect(cam_to_frag, f_norm);

    vec3 reflect_color = vec3(0.0);
    #if (REFLECTION_MODE >= REFLECTION_MODE_MEDIUM)
        reflect_color = get_sky_color(reflect_ray_dir, f_pos, vec3(-100000), 0.125, true, 1.0, true, sun_shade_frac);
    #endif

    vec3 emitted_light, reflected_light;

    // Prevent the sky affecting light when underground
    float not_underground = clamp((f_pos.z - f_alt) / 128.0 + 1.0, 0.0, 1.0);
    reflect_color *= not_underground;

    float max_light = 0.0;
    max_light += get_sun_diffuse2(sun_info, moon_info, f_norm, sun_view_dir, f_pos, mu, cam_attenuation, fluid_alt, k_a, k_d, k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);

    emitted_light *= not_underground;
    reflected_light *= not_underground;

    // Global illumination when underground (silly)
    emitted_light += (1.0 - not_underground) * 0.05;

    float point_shadow = shadow_at(f_pos, f_norm);
    reflected_light *= point_shadow;
    emitted_light *= point_shadow;

    max_light += lights_at(f_pos, cam_norm, view_dir, mu, cam_attenuation, fluid_alt,  k_a, k_d, k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);
    
    float passthrough = max(dot(cam_norm, -cam_to_frag), 0);

    float min_refl = 0.0;
    float opacity = (1.0 - passthrough) * 1.0 / (1.0 + min_refl);
    if (medium.x == MEDIUM_WATER) {
        // Hack to make the transparency of the surface fade when underwater to avoid artifacts
        opacity = min(sqrt(max(opacity, clamp((f_pos.z - cam_pos.z) * 0.05, 0.0, 1.0))), 0.99);
    }

    vec3 surf_color = illuminate(max_light, view_dir, water_color * emitted_light, reflect_color * water_shade + water_color * reflected_light);
    vec4 color = vec4(surf_color, opacity);

    tgt_color = color;
    tgt_mat = uvec4(uvec3((f_norm + 1.0) * 127.0), MAT_WATER);
#endif
}
