#include <lod.glsl>
#include <sky.glsl>

// Everything in here is entirely non-physical: it's the cheap fallback
vec3 get_cloud_color(vec3 surf_color, vec3 dir, vec3 origin, float max_dist, float quality) {
    // Underwater light attenuation
    surf_color = water_diffuse(surf_color, dir, max_dist);

    vec3 sky_light = get_sky_light(dir, false, 0.0);
    vec3 haze_color = mix(sky_light, sky_light * vec3(0.1, 0.3, 0.5), min(rain_density * 4, 1.0));
    
    #ifndef EXPERIMENTAL_NOHAZE
        float haze_factor = mix(0.00025, 0.01, rain_density);
        surf_color = mix(haze_color, surf_color, 1.0 / exp(min(max_dist, 8000.0) * haze_factor));
    #endif
    
    // This is rubbish... but fast
    float cloud_alt = cloud_avg_alt();
    if (dir.z * (cloud_alt - origin.z) > 0.0) {
        float dist = (cloud_alt - origin.z) / dir.z;
        if (dist < max_dist) {
            vec2 cloud_intersect = origin.xy + focus_off.xy + dir.xy * dist;
            surf_color = mix(
                surf_color,
                vec3(1 + noise_3d(vec3(cloud_intersect * 0.0001, time_of_day.x * 0.001)) * 2.0) * haze_color,
                min(cloud_tendency_at(cloud_intersect) * 30000 / dist, 1)
            );
        }
    }

    return surf_color;
}
