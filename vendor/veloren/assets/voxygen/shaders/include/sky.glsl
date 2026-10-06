#ifndef SKY_GLSL
#define SKY_GLSL

#include <random.glsl>
#include <srgb.glsl>
#include <shadows.glsl>
#include <globals.glsl>
#include <rain_occlusion.glsl>

// Information about an approximately directional light, like the sun or moon.
struct DirectionalLight {
    float shadow;
    // Fully blocks all light, including ambience
    float block;
};

const float PI = 3.141592653;

const vec3 SKY_DAWN_TOP = vec3(0.10, 0.1, 0.10);
const vec3 SKY_DAWN_MID = vec3(1.2, 0.3, 0.2);
const vec3 SKY_DAWN_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DAWN_LIGHT   = vec3(5.0, 2.0, 1.15);
const vec3 SUN_HALO_DAWN = vec3(3.0, 0.5, 0.1);

const vec3 SKY_DAY_TOP = vec3(0.1, 0.5, 0.9);
const vec3 SKY_DAY_MID = vec3(0.18, 0.28, 0.6);
const vec3 SKY_DAY_BOT = vec3(0.1, 0.2, 0.3);
const vec3 DAY_LIGHT   = vec3(3.8, 3.0, 1.8);
const vec3 SUN_HALO_DAY = vec3(0.15, 0.15, 0.001);

const vec3 SKY_DUSK_TOP = vec3(1.06, 0.1, 0.20);
const vec3 SKY_DUSK_MID = vec3(2.5, 0.3, 0.1);
const vec3 SKY_DUSK_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DUSK_LIGHT   = vec3(8.0, 1.5, 0.15);
const vec3 SUN_HALO_DUSK = vec3(3.0, 0.5, 0.05);

const vec3 SKY_NIGHT_TOP = vec3(0.001, 0.001, 0.0025);
const vec3 SKY_NIGHT_MID = vec3(0.001, 0.005, 0.02);
const vec3 SKY_NIGHT_BOT = vec3(0.002, 0.004, 0.004);
const vec3 NIGHT_LIGHT   = vec3(5.0, 0.75, 0.2);

// Linear RGB, scattering coefficients for atmosphere at roughly R, G, B wavelengths.
//
// See https://en.wikipedia.org/wiki/Diffuse_sky_radiation
const vec3 MU_SCATTER = vec3(0.05, 0.10, 0.23);

const float SUN_COLOR_FACTOR = 5.0;
const float MOON_COLOR_FACTOR = 5.0;

const float UNDERWATER_MIST_DIST = 100.0;

const float PERSISTENT_AMBIANCE = 1.0 / 32.0;

// Glow from static light sources
// Allowed to be > 1 due to HDR
const vec3 GLOW_COLOR = vec3(0.89, 0.95, 0.52);

// Calculate glow from static light sources, + some noise for flickering.
// TODO: Optionally disable the flickering for performance?
vec3 glow_light(vec3 pos) {
    #if (SHADOW_MODE <= SHADOW_MODE_NONE)
        return GLOW_COLOR;
    #else
        return GLOW_COLOR * (1.0 + (noise_3d(vec3(pos.xy * 0.005, tick.x * 0.5)) - 0.5) * 0.5);
    #endif
}

float cloud_avg_alt() { return view_distance.z + (view_distance.w - view_distance.z) * 1.25; }

const float wind_speed = 0.25;
vec2 wind_offset() { return vec2(time_of_day.y * wind_speed * (3600.0 * 24.0)); }

float cloud_scale() { return view_distance.z / 150.0; }

layout(set = 0, binding = 5) uniform texture2D t_alt;
layout(set = 0, binding = 6) uniform sampler s_alt;

// Transforms coordinate in the range 0..WORLD_SIZE to 0..1
vec2 wpos_to_uv(vec2 wpos) {
    // Want: (pixel + 0.5) / W
    vec2 texSize = textureSize(sampler2D(t_alt, s_alt), 0);
    vec2 uv_pos = (wpos + 16) / (32.0 * texSize);
    return vec2(uv_pos.x, uv_pos.y);
}

// Weather texture
layout(set = 0, binding = 12) uniform texture2D t_weather;
layout(set = 0, binding = 13) uniform sampler s_weather;

vec4 sample_weather(vec2 wpos) {
    return textureLod(sampler2D(t_weather, s_weather), wpos_to_uv(wpos), 0);
}

float cloud_tendency_at(vec2 wpos) {
    return sample_weather(wpos).r;
}

float rain_density_at(vec2 wpos) {
    return sample_weather(wpos).g;
}

float cloud_shadow(vec3 pos, vec3 light_dir) {
    #if (CLOUD_MODE <= CLOUD_MODE_MINIMAL)
        return 1.0;
    #else
        vec2 xy_offset = light_dir.xy * ((cloud_avg_alt() - pos.z) / -light_dir.z);

        // Fade out shadow if the sun angle is too steep (simulates a widening penumbra with distance)
        const vec2 FADE_RANGE = vec2(1500, 10000);
        float fade = 1.0 - clamp((length(xy_offset) - FADE_RANGE.x) / (FADE_RANGE.y - FADE_RANGE.x), 0, 1);
        float cloud = cloud_tendency_at(pos.xy + focus_off.xy - xy_offset);

        return clamp(1 - fade * cloud * 16.0, 0, 1);
    #endif
}

float magnetosphere() { return sin(time_of_day.y); }

vec3 magnetosphere_tint() {
    #if (CLOUD_MODE <= CLOUD_MODE_LOW)
        return vec3(1);
    #else
        float magnetosphere = magnetosphere();
        float magnetosphere2 = pow(magnetosphere, 2) * 2 - 1;
        float magnetosphere3 = pow(magnetosphere2, 2) * 2 - 1;
        vec3 magnetosphere_change = vec3(1.0) + vec3(
            (magnetosphere + 1.0) * 2.0,
            (-magnetosphere2 + 1.0) * 2.0,
            (-magnetosphere3 + 1.0) * 1.0
        ) * 0.4;
        return normalize(magnetosphere_change);
    #endif
 }

#if (CLOUD_MODE > CLOUD_MODE_FLAT)
float emission_strength() {
    return clamp((magnetosphere() - 0.3) * 1.3, 0, 1) * max(sun_dir.z, 0);
}

float emission_br() {
    #if (CLOUD_MODE >= CLOUD_MODE_MEDIUM)
        return abs(pow(fract(time_of_day.y * 0.5) * 2 - 1, 2));
    #else
        return 0.5;
    #endif
}
#endif


float get_sun_brightness() {
    return max(-sun_dir.z + 0.5, 0.0);
}

float get_moon_brightness() {
    return max(sun_dir.z + 0.6, 0.0) * 0.1;
}

vec3 get_sun_color() {
    vec3 light = (sun_dir.x > 0) ? DUSK_LIGHT : DAWN_LIGHT;

    return mix(
        mix(
            light * magnetosphere_tint(),
            NIGHT_LIGHT,
            max(sun_dir.z, 0)
        ),
        DAY_LIGHT,
        max(-sun_dir.z, 0)
    );
}

// Average sky colour (i.e: perfectly scattered light from the sky)
vec3 get_sky_color() {
    return mix(
        mix(
            (SKY_DUSK_TOP + SKY_DUSK_MID) / 2 * magnetosphere_tint(),
            (SKY_NIGHT_TOP + SKY_NIGHT_MID) / 2,
            max(sun_dir.z, 0)
        ),
        (SKY_DAY_TOP + SKY_DAY_MID) / 2,
        max(-sun_dir.z, 0)
    );
}

vec3 get_moon_color() {
    return vec3(0.5, 0.5, 1.6);
}

DirectionalLight get_sun_info(vec4 _dir, float shade_frac, vec3 f_pos) {
    float shadow = shade_frac;
    float block = 1.0;
#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        if (sun_dir.z < 0.0) {
            shadow = min(shadow, ShadowCalculationDirected(f_pos));
        }
    #endif
#endif
    return DirectionalLight(shadow, block);
}

DirectionalLight get_moon_info(vec4 _dir, float shade_frac) {
    float shadow = shade_frac;
    float block = 1.0;
    return DirectionalLight(shadow, block);
}

const float LIGHTNING_HEIGHT = 25.0;
const float MAX_LIGHTNING_PERIOD = 5.0;

float lightning_intensity() {
    float time_since_lightning = time_since(last_lightning.w);
    return
        // Strength
        1000000
        // Flash
        * max(0.0, 1.0 - time_since_lightning * 1.0)
        // Reverb
        * max(sin(time_of_day.x * 0.4), 0.0);
}

vec3 lightning_at(vec3 wpos) {
    float time_since_lightning = time_since(last_lightning.w);
    if (time_since_lightning < MAX_LIGHTNING_PERIOD) {
        vec3 diff = wpos + focus_off.xyz - (last_lightning.xyz + vec3(0, 0, LIGHTNING_HEIGHT));
        float dist = length(diff);
        return vec3(0.5, 0.8, 1.0)
            * lightning_intensity()
            // Attenuation
            / pow(50.0 + dist, 2);
    } else {
        return vec3(0.0);
    }
}

// Returns computed maximum intensity.
//
// wpos is the position of this fragment.
// mu is the attenuation coefficient for any substance on a horizontal plane.
// cam_attenuation is the total light attenuation due to the substance for beams between the point and the camera.
// surface_alt is the altitude of the attenuating surface.
float get_sun_diffuse2(
    DirectionalLight sun_info,
    DirectionalLight moon_info,
    vec3 norm,
    vec3 dir,
    vec3 wpos,
    vec3 mu,
    vec3 cam_attenuation,
    float surface_alt,
    vec3 k_a,
    vec3 k_d,
    vec3 k_s,
    float alpha,
    vec3 voxel_norm,
    float voxel_lighting,
    out vec3 emitted_light,
    out vec3 reflected_light
) {
    const vec3 SUN_AMBIANCE = MU_SCATTER;
    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        const vec3 MOON_AMBIANCE = MU_SCATTER;
    #else
        // Boost ambiance, because we don't properly compensate for pupil dilation (which should occur *before* HDR,
        // not in the end user's eye). Also, real nights are too dark to be fun.
        const vec3 MOON_AMBIANCE = vec3(0.15, 0.25, 0.23) * 5;
    #endif

    vec3 sun_dir = sun_dir.xyz;
    // TODO: Use real moon dir here and have other ways to light up night.
    // So this is a hack to just pretend the moon is still opposite to the sun
    // for this and `get_moon_brightness`.
    vec3 moon_dir = -sun_dir.xyz;

    float sun_light = get_sun_brightness() * sun_info.block;
    float moon_light = get_moon_brightness() * moon_info.block * ambiance;

    vec3 sun_color = get_sun_color() * SUN_COLOR_FACTOR;
    vec3 moon_color = get_moon_color() * MOON_COLOR_FACTOR;

    // If the sun is facing the wrong way, we currently just want zero light, hence default point is wpos.
    vec3 sun_attenuation = compute_attenuation(wpos, -sun_dir, mu, surface_alt, wpos);
    vec3 moon_attenuation = compute_attenuation(wpos, -moon_dir, mu, surface_alt, wpos);

    vec3 sun_chroma = sun_color * sun_light * cam_attenuation * sun_attenuation;
    vec3 moon_chroma = moon_color * moon_light * cam_attenuation * moon_attenuation;

    float sun_shadow = sun_info.shadow * cloud_shadow(wpos, sun_dir);
    float moon_shadow = moon_info.shadow * cloud_shadow(wpos, moon_dir);

    // https://en.m.wikipedia.org/wiki/Diffuse_sky_radiation
    //
    // HdRd radiation should come in at angle normal to us.
    // const float H_d = 0.23;
    //
    // Let β be the angle from horizontal
    // (for objects exposed to the sky, where positive when sloping towards south and negative when sloping towards north):
    //
    //     sin β = (north ⋅ norm) / |north||norm|
    //           = dot(vec3(0, 1, 0), norm)
    //
    //     cos β = sqrt(1.0 - dot(vec3(0, 1, 0), norm))
    //
    // Let h be the hour angle (180/0.0 at midnight, 90/1.0 at dawn, 0/0.0 at noon, -90/-1.0 at dusk, -180 at midnight/0.0):
    //     cos h = (midnight ⋅ -light_dir) / |midnight||-light_dir|
    //           = (noon ⋅ light_dir) / |noon||light_dir|
    //           = dot(vec3(0, 0, 1), light_dir)
    //
    // Let φ be the latitude at this point. 0 at equator, -90 at south pole / 90 at north pole.
    //
    // Let δ be the solar declination (angular distance of the sun's rays north [or south[]
    // of the equator), i.e. the angle made by the line joining the centers of the sun and Earth with its projection on the
    // equatorial plane.  Caused by axial tilt, and 0 at equinoxes.  Normally varies between -23.45 and 23.45 degrees.
    //
    // Let α (the solar altitude / altitud3 angle) be the vertical angle between the projection of the sun's rays on the
    // horizontal plane and the direction of the sun's rays (passing through a point).
    //
    // Let Θ_z be the vertical angle between sun's rays and a line perpendicular to the horizontal plane through a point,
    // i.e.
    //
    // Θ_z = (π/2) - α
    //
    // i.e. cos Θ_z = sin α and
    //      cos α = sin Θ_z
    //
    // Let γ_s be the horizontal angle measured from north to the horizontal projection of the sun's rays (positive when
    // measured westwise).
    //
    // cos Θ_z = cos φ cos h cos δ + sin φ sin δ
    // cos γ_s = sec α (cos φ sin δ - cos δ sin φ cos h)
    //         = (1  / √(1 - cos² Θ_z)) (cos φ sin δ - cos δ sin φ cos h)
    // sin γ_s = sec α cos δ sin h
    //         = (1 / cos α) cos δ sin h
    //         = (1 / sin Θ_z) cos δ sin h
    //         = (1  / √(1 - cos² Θ_z)) cos δ sin h
    //
    // R_b = (sin(δ)sin(φ - β) + cos(δ)cos(h)cos(φ - β))/(sin(δ)sin(φ) + cos(δ)cos(h)cos(φ))
    //
    // Assuming we are on the equator (i.e. φ = 0), and there is no axial tilt or we are at an equinox (i.e. δ = 0):
    //
    // cos Θ_z = 1 * cos h * 1 + 0 * 0 = cos h
    // cos γ_s = (1  / √(1 - cos² h)) (1 * 0 - 1 * 0 * cos h)
    //         = (1  / √(1 - cos² h)) * 0
    //         = 0
    // sin γ_s = (1  / √(1 - cos² h)) * sin h
    //         = sin h / sin h
    //         = 1
    //
    // R_b = (0 * sin(0 - β) + 1 * cos(h) * cos(0 - β))/(0 * 0 + 1 * cos(h) * 1)
    //     = (cos(h)cos(-β)) / cos(H)
    //     = cos(-β), the angle from horizontal.
    //
    // NOTE: cos(-β) = cos(β).
    // float cos_sun = dot(norm, /*-sun_dir*/vec3(0, 0, 1));
    // float cos_moon = dot(norm, -moon_dir);
    //
    // Let ζ = diffuse reflectance of surrounding ground for solar radiation, then we have
    //
    // R_d = (1 + cos β) / 2
    // R_r = ζ (1 - cos β) / 2
    //
    // H_t = H_b R_b + H_d R_d + (H_b + H_d) R_r
    float sin_beta = dot(vec3(0, 1, 0), norm);
    float R_b = sqrt(max(0.0, 1.0 - sin_beta * sin_beta));
    // Rough estimate of diffuse reflectance of rest of ground.
    // NOTE: zeta should be close to 0.7 with snow cover, 0.2 normally?  Maybe?
    vec3 zeta = max(vec3(0.2), k_d * (1.0 - k_s));
    float R_d = (1 + R_b) * 0.5;
    vec3 R_r = zeta * (1.0 - R_b) * 0.5;
    //
    // We can break this down into:
    //      H_t_b = H_b * (R_b + R_r) = light_intensity * (R_b + R_r)
    //      H_t_r = H_d * (R_d + R_r) = light_intensity * (R_d + R_r)
    vec3 R_t_b = R_b + R_r;
    vec3 R_t_r = R_d + R_r;

    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        vec3 lrf = light_reflection_factor(norm, dir, -norm, k_d, vec3(0.0), alpha, voxel_norm, voxel_lighting);
    #else
        // In practice, for gameplay purposes, we often want extra light at earlier and later times, so we use a
        // non-physical LRF to boost light during dawn and dusk.
        float lrf = pow(dot(norm, vec3(0, 0, 1)) + 1, 2) * 0.25;
    #endif
    vec3 light_frac = R_t_b * (sun_chroma * SUN_AMBIANCE + moon_chroma * MOON_AMBIANCE) * lrf;

    emitted_light = light_frac;

    vec3 emission = vec3(0);
    #if (CLOUD_MODE > CLOUD_MODE_FLAT)
        if (emission_strength() > 0.0) {
            emission = mix(vec3(0, 0.5, 1), vec3(1, 0, 0), emission_br()) * emission_strength() * 0.025;
        }
    #endif

    #ifdef FLASHING_LIGHTS_ENABLED
        vec3 lightning = lightning_at(wpos);
    #else
        vec3 lightning = vec3(0);
    #endif

    reflected_light = R_t_r * (
        (1.0 - SUN_AMBIANCE) * sun_chroma * sun_shadow * light_reflection_factor(norm, dir, sun_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + (1.0 - MOON_AMBIANCE) * moon_chroma * moon_shadow * light_reflection_factor(norm, dir, moon_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + emission
    ) + lightning;

    return rel_luminance(emitted_light + reflected_light);
}

// This has been extracted into a function to allow quick exit when detecting a star.
float is_star_at(vec3 dir) {
    float star_scale = 80.0;

    // Star positions
    vec3 pos = (floor(dir * star_scale) - 0.5) / star_scale;

    // Noisy offsets
    pos += (3.0 / star_scale) * (1.0 + hash(pos.yxzz) * 0.85);

    // Find distance to fragment
    float dist = length(pos - dir);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        const float power = 5.0;
    #else
        const float power = 50.0;
    #endif
    return power * max(sun_dir.z, 0.1) / (1.0 + pow(dist * 750, 8));
}

vec3 get_sky_light(vec3 dir, bool with_stars, float is_moon) {
    // Add white dots for stars. Note these flicker and jump due to FXAA
    float star = 0.0;
    if (with_stars) {
        vec3 star_dir = sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y;
        star = is_star_at(star_dir) * (1.0 - is_moon);
    }

    vec3 sky_twilight_top = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_mid = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_bot = vec3(0.0, 0.0, 0.0);
    if (sun_dir.x > 0) {
      sky_twilight_top = SKY_DUSK_TOP;
      sky_twilight_mid = SKY_DUSK_MID;
      sky_twilight_bot = SKY_DUSK_BOT;
    } else {
      sky_twilight_top = SKY_DAWN_TOP;
      sky_twilight_mid = SKY_DAWN_MID;
      sky_twilight_bot = SKY_DAWN_BOT;
    }

    vec3 sky_top = mix(
        mix(
            sky_twilight_top * magnetosphere_tint(),
            SKY_NIGHT_TOP,
            pow(max(sun_dir.z, 0.0), 0.2)
        ) + star,
        SKY_DAY_TOP,
        max(-sun_dir.z, 0)
    );

    vec3 sky_mid = mix(
        mix(
            sky_twilight_mid * magnetosphere_tint(),
            SKY_NIGHT_MID,
            pow(max(sun_dir.z, 0.0), 0.1)
        ),
        SKY_DAY_MID,
        max(-sun_dir.z, 0)
    );

    vec3 sky_bot = mix(
        mix(
            sky_twilight_bot * magnetosphere_tint(),
            SKY_NIGHT_BOT,
            pow(max(sun_dir.z, 0.0), 0.2)
        ),
        SKY_DAY_BOT,
        max(-sun_dir.z, 0)
    );

    vec3 sky_color = mix(
        mix(
            sky_mid,
            sky_bot,
            max(-dir.z, 0)
        ),
        sky_top,
        max(dir.z, 0)
    );

    return sky_color * magnetosphere_tint();
}

vec3 get_sky_color(vec3 dir, vec3 origin, vec3 f_pos, float quality, bool with_features, float refractionIndex, bool fake_clouds, float sun_shade_frac) {
    // Sky color
    vec3 sun_dir = sun_dir.xyz;
    vec3 moon_dir = moon_dir.xyz;


    // Sun
    const vec3 SUN_SURF_COLOR = vec3(1.5, 0.9, 0.35) * 10.0;

    vec3 sun_halo_color = mix(
        (sun_dir.x > 0 ? SUN_HALO_DUSK : SUN_HALO_DAWN)* magnetosphere_tint(),
        SUN_HALO_DAY,
        pow(max(-sun_dir.z, 0.0), 0.5)
    );

    float sun_halo_power = 20.0;
    if (fake_clouds || medium.x == MEDIUM_WATER) {
        sun_halo_power = 30.0;
        sun_halo_color *= 0.01;
    }

    vec3 sun_halo = sun_halo_color * 25 * pow(max(dot(dir, -sun_dir), 0), sun_halo_power);
    vec3 sun_surf = vec3(0);
    if (with_features) {
        float angle = 0.00035;
        sun_surf = clamp((dot(dir, -sun_dir) - (1.0 - angle)) * 4 / angle, 0, 1)
            * SUN_SURF_COLOR
            * SUN_COLOR_FACTOR
            * sun_shade_frac;
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sun_surf *= 0.1;
    }
    vec3 sun_light = sun_halo + sun_surf;

    // Moon
    const vec3 MOON_SURF_COLOR = vec3(0.7, 1.0, 1.5) * 250.0;
    const vec3 MOON_HALO_COLOR = vec3(0.015, 0.015, 0.05) * 250;

    vec3 moon_halo_color = MOON_HALO_COLOR;

    float moon_halo_power = 20.0;

    vec3 moon_surf = vec3(0);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_power = 50.0;
    }

    float is_moon = 0.0;

    if (with_features) {
        float moon_radius = 0.035;
        
        float tca = dot(-moon_dir, dir);

        float radius2 = moon_radius * moon_radius;
        float d2 = 1.0 - tca * tca;

        float diff = radius2 - d2;

        is_moon = clamp(tca * 2000.0, 0.0, 1.0) * clamp(diff * 4000.0, 0.0, 1.0);

        if (is_moon > 0.0) {
            float thc = sqrt(diff);

            float t0 = tca - thc;

            vec3 moon_normal = (t0 * dir + moon_dir) * (1.0 / moon_radius);

            float noise = snoise3(moon_normal * 8.2) + snoise3(moon_normal * 25.0) * 0.4;

            float direct_sunlight = max(dot(moon_normal, -sun_dir), 0.0);

            float planet_albedo = 0.12;
            float planet_reflected_light = (1.0 - abs(dot(moon_dir, sun_dir))) * max(dot(moon_normal, moon_dir), 0.0) * planet_albedo;
            float light = max(direct_sunlight + planet_reflected_light, 0.001);

            // ~sun is the same direction from the moon as it is to us.
            float surface_light = pow(light * (0.4 + 0.3 * noise), 2.0);

            moon_surf = MOON_SURF_COLOR * surface_light * is_moon;
        }
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_color *= 0.2;
        moon_surf *= 0.05;
    }
    vec3 moon_halo = moon_halo_color * pow(max(dot(dir, -moon_dir), 0) * max(dot(sun_dir, -moon_dir), 0), moon_halo_power);
    vec3 moon_light = moon_halo + moon_surf;

    // Replaced all clamp(sun_dir, 0, 1) with max(sun_dir, 0) because sun_dir is calculated from sin and cos, which are never > 1

    vec3 sky_color;
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sky_color = get_sky_light(dir, !fake_clouds, is_moon);
    } else {
        if (medium.x == MEDIUM_WATER) {
            sky_color = get_sky_light(dir, true, is_moon);
        } else {
            vec3 star_dir = normalize(sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y);
            float star = is_star_at(star_dir) * (1.0 - is_moon);
            sky_color = vec3(0) + star;
        }
    }

    return sky_color + sun_light + moon_light;
}

float fog(vec3 f_pos, vec3 focus_pos, uint medium) {
    return max(1.0 - 5000.0 / (1.0 + distance(f_pos.xy, focus_pos.xy)), 0.0);
}

vec3 illuminate(float max_light, vec3 view_dir, vec3 emitted, vec3 reflected) {
    return emitted + reflected;
}

vec3 simple_lighting(vec3 pos, vec3 col, float shade) {
    // Bad fake lantern so we can see in caves
    vec3 d = pos.xyz - focus_pos.xyz;
    return col * clamp(2.5 / dot(d, d), shade * (get_sun_brightness() + 0.01), 1);
}

float wind_wave(float off, float scaling, float speed, float strength) {
    float aspeed = abs(speed);

    // TODO: Right now, the wind model is pretty simplistic. This means that there is frequently no wind at all, which
    // looks bad. For now, we add a lower bound on the wind speed to keep things looking nice.
    strength = max(strength, 6.0);
    aspeed = max(aspeed, 5.0);

    return (sin(tick_loop(2.0 * PI, 0.35 * scaling * floor(aspeed), off)) * (1.0 - fract(aspeed))
        + sin(tick_loop(2.0 * PI, 0.35 * scaling * ceil(aspeed), off)) * fract(aspeed)) * abs(strength) * 0.25;
}

#endif
