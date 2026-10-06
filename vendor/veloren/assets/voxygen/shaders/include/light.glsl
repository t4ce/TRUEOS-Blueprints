#ifndef LIGHT_GLSL
#define LIGHT_GLSL

#include <srgb.glsl>
#include <shadows.glsl>
#include <random.glsl>

struct Light {
    vec4 light_pos;
    vec4 light_col;
    vec4 light_dir; // w is fov
    // mat4 light_proj;
};

layout (std140, set = 0, binding = 3)
uniform u_lights {
    // TODO: insert light max count constant here when loading the shaders
    Light lights[20];
};

struct Shadow {
    vec4 shadow_pos_radius;
};

layout (std140, set = 0, binding = 4)
uniform u_shadows {
    Shadow shadows[24];
};

float attenuation_strength(vec3 rpos) {
    // This is not how light attenuation works at all, but it produces visually pleasing and mechanically useful properties
    float d2 = rpos.x * rpos.x + rpos.y * rpos.y + rpos.z * rpos.z;
    return max(2.0 / pow(d2 + 10, 0.35) - pow(d2 / 50000.0, 0.8), 0.0);
}

float attenuation_strength_real(vec3 rpos) {
    float d2 = rpos.x * rpos.x + rpos.y * rpos.y + rpos.z * rpos.z;
    return 1.0 / (0.025 + d2);
}

vec3 light_at(vec3 wpos, vec3 wnorm) {
    const float LIGHT_AMBIANCE = 0.025;

    vec3 light = vec3(0);

    for (uint i = 0u; i < light_shadow_count.x; i ++) {

        // Only access the array once
        Light L = lights[i];

        vec3 light_pos = L.light_pos.xyz - focus_off.xyz;

        // Pre-calculate difference between light and fragment
        vec3 difference = light_pos - wpos;

        float strength = attenuation_strength(difference);

        vec3 color = L.light_col.rgb * strength;

        light += color * (max(0, max(dot(normalize(difference), wnorm), 0.15)) + LIGHT_AMBIANCE);
    }
    return light;
}

float shadow_at(vec3 wpos, vec3 wnorm) {
    float shadow = 1.0;

#if (SHADOW_MODE == SHADOW_MODE_CHEAP || (SHADOW_MODE == SHADOW_MODE_MAP && defined(EXPERIMENTAL_POINTSHADOWSWITHSHADOWMAPPING)))
    for (uint i = 0u; i < light_shadow_count.y; i ++) {

        // Only access the array once
        Shadow S = shadows[i];

        vec3 shadow_pos = S.shadow_pos_radius.xyz - focus_off.xyz;
        float radius = S.shadow_pos_radius.w;

        vec3 diff = shadow_pos - wpos;
        #if (SHADOW_MODE == SHADOW_MODE_CHEAP)
            if (diff.z >= 0.0) {
                diff.z = -sign(diff.z) * diff.z * 0.1;
            }
        #endif

        float shade = max(pow(diff.x * diff.x + diff.y * diff.y + diff.z * diff.z, 0.3) / pow(radius * radius * 0.5, 0.5), 0.5);

        shadow = min(shadow, shade);
    }
    return min(shadow, 1.0);
#else
    return shadow;
#endif
}

// Returns computed maximum intensity.
//
// mu is the attenuation coefficient for any substance on a horizontal plane.
// cam_attenuation is the total light attenuation due to the substance for beams between the point and the camera.
// surface_alt is the altitude of the attenuating surface.
float lights_at(vec3 wpos, vec3 wnorm, vec3 /*cam_to_frag*/view_dir, vec3 mu, vec3 cam_attenuation, float surface_alt, vec3 k_a, vec3 k_d, vec3 k_s, float alpha, vec3 voxel_norm, float voxel_lighting, inout vec3 emitted_light, inout vec3 reflected_light/*, out float shadow*/) {
    vec3 directed_light = vec3(0.0);
    vec3 max_light = vec3(0.0);

    const float LIGHT_AMBIANCE = 0.0;

    for (uint i = 0u; i < light_shadow_count.x; i ++) {

        // Only access the array once
        Light L = lights[i];

        vec3 light_pos = L.light_pos.xyz - focus_off.xyz;

        // Pre-calculate difference between light and fragment
        vec3 difference = light_pos - wpos;
        float distance_2 = dot(difference, difference);

        if (distance_2 > 100000.0) {
            continue;
        }
        
        // NOTE: This normalizes strength to 0.25 at the center of the point source.
        float dist_strength = 3.0 / (5 + distance_2);

        float strength = dist_strength;

        // Multiply the vec3 only once
        const float PI = 3.1415926535897932384626433832795;
        const float PI_2 = 2 * PI;
        vec3 color = /*srgb_to_linear*/L.light_col.rgb;

        // Compute reflectance.
        float light_distance = sqrt(distance_2);
        vec3 light_dir = -difference / light_distance;
        bool is_direct = true;
        vec3 direct_light_dir = is_direct ? light_dir : -light_dir;
        
        // Directional light
        if (L.light_dir.w < 1.0) {
            strength *= clamp(
                // Stength increases toward light direction past the light fov threshold...
                (dot(light_dir, L.light_dir.xyz) - L.light_dir.w) / (1.0 - L.light_dir.w) * 8 + 2.5,
                // ...but is clamped to minimum and maximum brightness
                0.1 * (1.0 - L.light_dir.w),
                1.0
            );
            // Ambient strength also decays for directional lights, but less severely than the beam
            dist_strength *= pow(dot(L.light_dir.xyz, light_dir) * 0.5 + 0.5, L.light_dir.w * 10.0);
        }
        
        // Compute attenuation due to fluid.
        // Default is light_pos, so we take the whole segment length for this beam if it never intersects the surface, unlesss the beam itself
        // is above the surface, in which case we take zero (wpos).
        color *= cam_attenuation * compute_attenuation_point(wpos, -direct_light_dir, mu, surface_alt, light_pos.z < surface_alt ? light_pos : wpos);

#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        is_direct = true;
#endif
        vec3 lrf = light_reflection_factor(wnorm, view_dir, direct_light_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting);
        vec3 direct_light = PI * color * strength * lrf;
        float computed_shadow = ShadowCalculationPoint(i, -difference, wnorm, wpos);
        float ambiance = 0.0;
        #ifndef EXPERIMENTAL_PHOTOREALISTIC
            // Non-physically emulate ambient light nearby
            ambiance = 0.25 * (dot(wnorm, direct_light_dir) * 0.3 + 0.7) * dist_strength;
            #ifdef FIGURE_SHADER
                // Non-physical hack. Subtle, but allows lanterns to glow nicely
                // TODO: Make lanterns use glowing cells instead
                ambiance += 0.25 * dist_strength / (0.001 + pow(distance_2 * 10.0, 1.5));
            #endif
        #endif
        directed_light += (is_direct ? mix(LIGHT_AMBIANCE, 1.0, computed_shadow) * direct_light : vec3(0.0)) + ambiance * color;

        vec3 cam_light_diff = light_pos - focus_pos.xyz;
        float cam_distance_2 = dot(cam_light_diff, cam_light_diff);
        float cam_strength = 1.0 / cam_distance_2;

        float both_strength = cam_distance_2 == 0.0 ? distance_2 == 0.0 ? 0.0 : strength : distance_2 == 0.0 ? cam_strength :
            cam_strength + strength;
        max_light += computed_shadow * both_strength * PI * color;
    }

    reflected_light += directed_light;
    return rel_luminance(max_light);
}

// Same as lights_at, but with no assumed attenuation due to fluid.
float lights_at(vec3 wpos, vec3 wnorm, vec3 view_dir, vec3 k_a, vec3 k_d, vec3 k_s, float alpha, inout vec3 emitted_light, inout vec3 reflected_light) {
    return lights_at(wpos, wnorm, view_dir, vec3(0.0), vec3(1.0), 0.0, k_a, k_d, k_s, alpha, wnorm, 1.0, emitted_light, reflected_light);
}

void apply_cell_material(
    uint material,
    in vec3 f_pos,
    in vec3 f_norm,
    in vec3 surf_color,
    inout vec3 emitted_light,
    // Mostly used to communicate reflectivity
    inout float render_alpha,
    inout uint render_mat
) {
    vec3 wpos = f_pos + focus_off.xyz;
    // Apply material surface properties
    switch (material & 31u) {
        // Glowy
        case 1:
            emitted_light += 20 * surf_color;
            break;
        // Shiny
        case 2:
            render_alpha = 0.1;
            break;
        // Fire
        case 3:
            emitted_light += surf_color * 5.0;
            emitted_light *= 32.0 * (0.02 + pow(noise_3d(vec3(wpos.xy * 2.0, 100000.0 - wpos.z * 2.0 + tick.x * 5.0) * 0.1), 3.0));
            break;
        // Water
        case 4:
            render_alpha = 0.2;
            render_mat = MAT_PUDDLE;
            break;
        // SwirlyCrystal
        case 5:
            vec3 dpos = vec3(1000.0) - wpos;
            emitted_light = mix(
                pow(mix(vec3(1.0, 1.2, 1.5), vec3(0.5, 0.3, 1.0), sin(tick.x * 0.1) * 0.5 + 0.5), vec3(7.0)),
                pow(mix(vec3(0.5, 1.0, 1.0), vec3(0.8, 0.5, 1.0), sin(tick.x * 0.13) * 0.5 + 0.5), vec3(7.0)),
                dot(sin(tick.xxx * 2.3 + dpos.zxy * vec3(30.0, 6.0, 6.0) + sin(dpos.zxy + sin(tick.x * 1.7 - dpos.z * 12.0) * 1.1) * 2.0) * 0.5 + 0.5, vec3(1.0)) / 3.0
            ) * 2.5 * (sin(tick.x * 3.0) * 0.4 + 1.0);
            break;
        default: break;
    }
}

#endif
