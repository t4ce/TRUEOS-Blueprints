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

// https://www.shadertoy.com/view/XdsyWf

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

#include <cloud.glsl>
#include <light.glsl>
#include <lod.glsl>

void wave_dx(vec4 posx, vec4 posy, vec2 dir, float speed, float frequency, float timeshift, out vec4 wave, out vec4 dx) {
    vec4 x = vec4(
        dot(dir, vec2(posx.x, posy.x)),
        dot(dir, vec2(posx.y, posy.y)),
        dot(dir, vec2(posx.z, posy.z)),
        dot(dir, vec2(posx.w, posy.w))
    ) * frequency + timeshift * speed;
    wave = sin(x) + 0.5;
    wave *= wave;
    dx = -wave * cos(x);
}

// Based loosely on https://www.shadertoy.com/view/MdXyzX.
// Modified to allow calculating the wave function 4 times at once using different positions (used for intepolation
// for moving water). The general idea is to sample the wave function at different positions, where those positions
// depend on increments of the velocity, and then interpolate between those velocities to get a smooth water velocity.
vec4 base_wave_height(vec4 posx, vec4 posy) {
    float iter = 0.0;
    float phase = 4.0;
    float weight = 1.5;
    vec4 w = vec4(0.0);
    float ws = 0.0;
    const float speed_per_iter = 0.1;
    #if (FLUID_MODE == FLUID_MODE_HIGH)
        float speed = 1.0;
        posx *= 0.2;
        posy *= 0.2;
        const float drag_factor = 0.035;
        const int iters = 21;
        const float scale = 15.0;
    #else
        float speed = 2.0;
        posx *= 0.3;
        posy *= 0.3;
        const float drag_factor = 0.04;
        const int iters = 11;
        const float scale = 3.0;
    #endif
    const float iter_shift = (3.14159 * 2.0) / 7.3;

    for(int i = 0; i < iters; i ++) {
        vec2 p = vec2(sin(iter), cos(iter));
        vec4 wave, dx;
        wave_dx(posx, posy, p, speed, phase, tick.z, wave, dx);
        posx += p.x * dx * weight * drag_factor;
        posy += p.y * dx * weight * drag_factor;
        w += wave * weight;
        iter += iter_shift * 1.5;
        ws += weight;
        weight = mix(weight, 0.0, 0.2);
        phase *= 1.2;
        speed += speed_per_iter;
    }
    return w / ws * scale;
}

vec4 wave_height(vec4 posx, vec4 posy, float z) {
    vec4 h = base_wave_height(
        mod(posx, vec4(3000.0)) + z * 0.2,
        mod(posy, vec4(3000.0)) + z * 0.2
    );

    #if (FLUID_MODE == FLUID_MODE_HIGH)
        for (uint i = 0u; i < light_shadow_count.y; i ++) {
            Shadow S = shadows[i];
            vec3 shadow_pos = S.shadow_pos_radius.xyz;
            float radius = S.shadow_pos_radius.w;

            vec4 dist = vec4(
                distance(vec3(posx.x, posy.x, z - 1), shadow_pos),
                distance(vec3(posx.y, posy.y, z - 1), shadow_pos),
                distance(vec3(posx.z, posy.z, z - 1), shadow_pos),
                distance(vec3(posx.w, posy.w, z - 1), shadow_pos)
            );

            h += sin(dist * 15.1 - tick.z * 15.0) / (pow(dist * radius, vec4(2.0)) + 1.0);
        }
    #endif

    return h;
}

float wave_height_vel(vec3 pos) {
    vec4 heights = wave_height(
        pos.x - tick.z * floor(f_vel.x) - vec2(0.0, tick.z).xyxy,
        pos.y - tick.z * floor(f_vel.y) - vec2(0.0, tick.z).xxyy,
        pos.z
    );
    return mix(
        mix(heights.x, heights.y, fract(f_vel.x + 1.0)),
        mix(heights.z, heights.w, fract(f_vel.x + 1.0)),
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
    // Temporarily assume all water faces up (this is incorrect but looks better)
    vec3 surf_norm = normals[norm_axis + norm_dir];
    vec3 f_norm = vec3(0, 0, 1);//surf_norm;
    vec3 cam_to_frag = normalize(f_pos - cam_pos.xyz);

    vec3 view_dir = -cam_to_frag;
    float frag_dist = length(f_pos - cam_pos.xyz);

    vec3 b_norm;
    if (f_norm.z > 0.0) {
        b_norm = vec3(1, 0, 0);
    } else if (f_norm.x > 0.0) {
        b_norm = vec3(0, 1, 0);
    } else {
        b_norm = vec3(0, 0, 1);
    }
    vec3 c_norm = cross(f_norm, b_norm);

    vec3 wave_pos = f_pos + focus_off.xyz;
    float wave_sample_dist = 0.1;
    float wave00 = wave_height_vel(wave_pos);
    float wave10 = wave_height_vel(wave_pos + vec3(wave_sample_dist, 0, 0));
    float wave01 = wave_height_vel(wave_pos + vec3(0, wave_sample_dist, 0));

    // Possibility of div by zero when slope = 0,
    // however this only results in no water surface appearing
    // and is not likely to occur (could not find any occurrences)
    float slope = abs((wave00 - wave10) * (wave00 - wave01)) + 0.001;

    vec3 nmap = vec3(
        -(wave10 - wave00) / wave_sample_dist,
        -(wave01 - wave00) / wave_sample_dist,
        wave_sample_dist / slope
    );

    #ifdef RAIN_ENABLED
        if (rain_density > 0 && surf_norm.z > 0.5) {
            vec3 drop_density = vec3(2, 2, 2);
            vec3 drop_pos = wave_pos + vec3(0, 0, -time_of_day.x * 0.025);
            vec2 cell2d = floor(drop_pos.xy * drop_density.xy);
            drop_pos.z += noise_2d(cell2d * 13.1) * 10;
            drop_pos.z *= 0.5 + hash_fast(uvec3(cell2d, 0));
            vec3 cell = vec3(cell2d, floor(drop_pos.z * drop_density.z));

            if (fract(hash(fract(vec4(cell, 0) * 0.01))) < rain_density * rain_occlusion_at(f_pos.xyz) * 50.0) {
                vec3 off = vec3(hash_fast(uvec3(cell * 13)), hash_fast(uvec3(cell * 5)), 0);
                vec3 near_cell = (cell + 0.5 + (off - 0.5) * 0.5) / drop_density;

                float dist = length((drop_pos - near_cell) / vec3(1, 1, 2));
                float drop_rad = 0.125;
                nmap.xy += (drop_pos - near_cell).xy
                    * max(1.0 - abs(dist - drop_rad) * 50, 0)
                    * 2500
                    * sign(dist - drop_rad)
                    * max(drop_pos.z - near_cell.z, 0);
            }
        }
    #endif

    nmap = mix(f_norm, normalize(nmap), min(1.0 / pow(frag_dist, 0.75), 1));
    vec3 norm = normalize(f_norm * nmap.z + b_norm * nmap.x + c_norm * nmap.y);

    vec3 water_color = (1.0 - MU_WATER) * MU_SCATTER;
#if (SHADOW_MODE == SHADOW_MODE_CHEAP || SHADOW_MODE == SHADOW_MODE_MAP || FLUID_MODE >= FLUID_MODE_MEDIUM)
    float f_alt = alt_at(f_pos.xy);
#elif (SHADOW_MODE == SHADOW_MODE_NONE || FLUID_MODE == FLUID_MODE_LOW)
    float f_alt = f_pos.z;
#endif

    float fluid_alt = mix(f_pos.z, f_alt, f_norm.z == 0);
    const float alpha = 0.255;
    const float n2 = 1.3325;
    const float R_s2s0 = pow(abs((1.0 - n2) / (1.0 + n2)), 2);
    const float R_s1s0 = pow(abs((1.3325 - n2) / (1.3325 + n2)), 2);
    const float R_s2s1 = pow(abs((1.0 - 1.3325) / (1.0 + 1.3325)), 2);
    const float R_s1s2 = pow(abs((1.3325 - 1.0) / (1.3325 + 1.0)), 2);
    float R_s = (f_pos.z < fluid_alt) ? mix(R_s2s1 * R_s1s0, R_s1s0, medium.x) : mix(R_s2s0, R_s1s2 * R_s2s0, medium.x);

    // Water is transparent so both normals are valid.
    vec3 cam_norm = faceforward(norm, norm, cam_to_frag);
    vec3 reflect_ray_dir = reflect(cam_to_frag, norm);
    vec3 refract_ray_dir = refract(cam_to_frag, norm, 1.0 / n2);
    vec3 sun_view_dir = view_dir;
    float f_light = 1.0;
    vec3 ray_dir;
    if (medium.x == MEDIUM_WATER) {
        ray_dir = refract(cam_to_frag, -norm, 1.33);
    } else {
        // Ensure the ray doesn't accidentally point underwater
        // TODO: Make this more efficient?
        ray_dir = normalize(max(reflect_ray_dir, vec3(-1.0, -1.0, 0.0)));
    }

#if (SHADOW_MODE == SHADOW_MODE_CHEAP || SHADOW_MODE == SHADOW_MODE_MAP)
    vec4 f_shadow = textureMaybeBicubic(t_horizon, s_horizon, pos_to_tex(f_pos.xy));
    float sun_shade_frac = horizon_at2(f_shadow, f_alt, f_pos, sun_dir);
#elif (SHADOW_MODE == SHADOW_MODE_NONE)
    float sun_shade_frac = 1.0;
#endif
    float moon_shade_frac = 1.0;

    vec3 reflect_color;
    #if (REFLECTION_MODE >= REFLECTION_MODE_MEDIUM)
        reflect_color = vec3(0);
    #else
        reflect_color = get_sky_color(ray_dir, f_pos, vec3(-100000), 0.125, true, 1.0, true, sun_shade_frac);
    #endif
    // Sort of non-physical, but we try to balance the reflection intensity with the direct light from the sun,
    // resulting in decent reflection of the ambient environment even after the sun has gone down.
    reflect_color *= f_light * (sun_shade_frac * 0.75 + 0.25);

    // Prevent the sky affecting light when underground
    float not_underground = clamp((f_pos.z - f_alt) / 32.0 + 1.0, 0.0, 1.0);
    reflect_color *= not_underground;

    DirectionalLight sun_info = get_sun_info(sun_dir, sun_shade_frac, f_pos);
    DirectionalLight moon_info = get_moon_info(moon_dir, moon_shade_frac);

    vec3 mu = MU_WATER;
    // NOTE: Default intersection point is camera position, meaning if we fail to intersect we assume the whole camera is in water.
    vec3 cam_attenuation = compute_attenuation_point(f_pos, -view_dir, mu, fluid_alt, cam_pos.xyz);
    
    vec3 k_a = vec3(1.0);
    vec3 k_d = vec3(1.0);
    vec3 k_s = vec3(0.0);

    vec3 emitted_light, reflected_light;
    // 0 = 100% reflection, 1 = translucent water
    float passthrough = max(dot(cam_norm, -cam_to_frag), 0) * 0.75;

    float max_light = 0.0;
    max_light += get_sun_diffuse2(sun_info, moon_info, cam_norm, sun_view_dir, f_pos, mu, cam_attenuation, fluid_alt, k_a, vec3(k_d), k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);
    emitted_light *= not_underground;
    reflected_light *= not_underground;

    // Global illumination when underground (silly)
    emitted_light += (1.0 - not_underground) * 0.05;

    float point_shadow = shadow_at(f_pos, f_norm);
    reflected_light *= point_shadow;
    
    max_light += lights_at(f_pos, cam_norm, view_dir, mu, cam_attenuation, fluid_alt, k_a, k_d, k_s, alpha, f_norm, 1.0, emitted_light, reflected_light);

    const float REFLECTANCE = 1.0;
    vec3 surf_color = illuminate(max_light, view_dir, water_color * emitted_light, reflect_color * REFLECTANCE + water_color * reflected_light);

    float min_refl = 0.0;
    float opacity = (1.0 - passthrough) * 0.5 / (1.0 + min_refl);
    if (medium.x != MEDIUM_WATER) {
        min_refl = min(emitted_light.r, min(emitted_light.g, emitted_light.b));
    } else {
        // Hack to make the transparency of the surface fade when underwater to avoid artifacts
        if (dot(refract_ray_dir, cam_to_frag) > 0.0) {
            opacity = 0.99;
        } else {
            opacity = min(sqrt(max(opacity, clamp((f_pos.z - cam_pos.z) * 0.05, 0.0, 1.0))), 0.99);
        }
    }

    tgt_color = vec4(surf_color, opacity);
    tgt_mat = uvec4(uvec3((norm + 1.0) * 127.0), MAT_WATER);
#endif
}
