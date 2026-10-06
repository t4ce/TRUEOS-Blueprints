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
// Note: The sampler uniform is declared here because it differs for MSAA
#include <anti-aliasing.glsl>
#include <srgb.glsl>
#include <cloud.glsl>
#include <random.glsl>
#include <lod.glsl>

layout(set = 1, binding = 0)
uniform texture2D t_src_color;
layout(set = 1, binding = 1)
uniform sampler s_src_color;

layout(set = 1, binding = 2)
uniform texture2D t_src_depth;
layout(set = 1, binding = 3)
uniform sampler s_src_depth;

layout(location = 0) in vec2 uv;

layout (std140, set = 1, binding = 4)
uniform u_locals {
    mat4 proj_mat_inv;
    mat4 view_mat_inv;
};

#ifdef BLOOM_FACTOR
layout(set = 1, binding = 5)
uniform texture2D t_src_bloom;
#ifdef EXPERIMENTAL_GRADIENTSOBEL
layout(set = 1, binding = 6)
uniform utexture2D t_src_mat;
#endif
#else
#ifdef EXPERIMENTAL_GRADIENTSOBEL
layout(set = 1, binding = 5)
uniform utexture2D t_src_mat;
#endif
#endif

layout(location = 0) out vec4 tgt_color;

vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));

    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

vec3 _illuminate(float max_light, vec3 view_dir, vec3 emitted, vec3 reflected) {
    const float gamma = 1.0;
    vec3 color = emitted + reflected;
    
    float lum = rel_luminance(color);
    float sky_light = lum;

    // Tone mapped value.
    // const float NIGHT_EXPOSURE = 10.0;
    // const float DUSK_EXPOSURE = 2.0;
    // const float DAY_EXPOSURE = 1.0;
    // float alpha = mix(
    //     mix(
    //         DUSK_EXPOSURE,
    //         NIGHT_EXPOSURE,
    //         max(sun_dir.z, 0)
    //     ),
    //     DAY_EXPOSURE,
    //     max(-sun_dir.z, 0)
    // );
    float alpha = sky_light > 0.0 && max_light > 0.0 ? mix(1.0 / log(1.0 + max_light / (0.0 + sky_light)), 1.0, clamp(max_light - sky_light, 0.0, 1.0)) : 1.0;

    vec3 col_adjusted = lum == 0.0 ? vec3(0.0) : color / lum;

    float T = 1.0 - exp(-alpha * lum);

    // Heuristic desaturation
    // const float DAY_SATURATION = 1.0;
    // const float DUSK_SATURATION = 0.6;
    // const float NIGHT_SATURATION = 0.1;
    // float s = mix(
    //     mix(
    //         DUSK_SATURATION,
    //         NIGHT_SATURATION,
    //         max(sun_dir.z, 0)
    //     ),
    //     DAY_SATURATION,
    //     max(-sun_dir.z, 0)
    // );
    float s = 1.0;

    return pow(col_adjusted, vec3(s)) * T;
}

#ifdef EXPERIMENTAL_SOBEL
vec3 aa_sample(vec2 uv, vec2 off) {
    return aa_apply(t_src_color, s_src_color, t_src_depth, s_src_depth, uv * screen_res.xy + off, screen_res.xy).rgb;
}
#endif
#ifdef EXPERIMENTAL_GRADIENTSOBEL
vec3 aa_sample_grad(vec2 uv, vec2 off) {
    uvec2 mat_sz = textureSize(usampler2D(t_src_mat, s_src_depth), 0);
    uvec4 mat = texelFetch(usampler2D(t_src_mat, s_src_depth), clamp(ivec2(uv * mat_sz + off), ivec2(0), ivec2(mat_sz) - 1), 0);
    return vec3(mat.xyz) / 255.0;
}
#endif

float dither(ivec2 p, float level) {
    // Bayer dithering
    int dither[8][8] = {
        { 0, 32, 8, 40, 2, 34, 10, 42}, /* 8x8 Bayer ordered dithering */
        {48, 16, 56, 24, 50, 18, 58, 26}, /* pattern. Each input pixel */
        {12, 44, 4, 36, 14, 46, 6, 38}, /* is scaled to the 0..63 range */
        {60, 28, 52, 20, 62, 30, 54, 22}, /* before looking in this table */
        { 3, 35, 11, 43, 1, 33, 9, 41}, /* to determine the action. */
        {51, 19, 59, 27, 49, 17, 57, 25},
        {15, 47, 7, 39, 13, 45, 5, 37},
        {63, 31, 55, 23, 61, 29, 53, 21}
    };
    return step((dither[p.x % 8][p.y % 8]+1) * 0.016, level);
}

void main() {
#ifdef EXPERIMENTAL_BAREMINIMUM
    tgt_color = vec4(texelFetch(sampler2D(t_src_color, s_src_color), ivec2(uv * textureSize(sampler2D(t_src_color, s_src_color), 0)), 0).rgb, 1);
#else

    vec2 c_uv = vec2(0.5);
    vec2 delta = min(uv, 1.0 - uv);
    delta = vec2(0.25);

    vec2 sample_uv = uv;
    #ifdef EXPERIMENTAL_UNDERWARPER
        if (medium.x == MEDIUM_WATER) {
            float x = tick_loop(2.0 * PI, 3.0, uv.y * 60);
            float y = tick_loop(2.0 * PI, 3.0, uv.x * 60);
            sample_uv += sin(vec2(x, y)) * 0.003;
        }
    #endif

    vec4 aa_color = aa_apply(t_src_color, s_src_color, t_src_depth, s_src_depth, sample_uv * screen_res.xy, screen_res.xy);

    #ifdef EXPERIMENTAL_SOBEL
        vec3 s[8];
        s[0] = aa_sample(uv, vec2(-1,  1));
        s[1] = aa_sample(uv, vec2( 0,  1));
        s[2] = aa_sample(uv, vec2( 1,  1));
        s[3] = aa_sample(uv, vec2(-1,  0));
        s[4] = aa_sample(uv, vec2( 1,  0));
        s[5] = aa_sample(uv, vec2(-1, -1));
        s[6] = aa_sample(uv, vec2( 0, -1));
        s[7] = aa_sample(uv, vec2( 1, -1));
        vec3 gx = s[0] + s[3] * 2.0 + s[5] - s[2] - s[4] * 2 - s[7];
        vec3 gy = s[0] + s[1] * 2.0 + s[2] - s[5] - s[6] * 2 - s[7];
        float mag = length(gx) + length(gy);
        aa_color.rgb = mix(vec3(0.9), aa_color.rgb * 0.8, clamp(1.0 - mag * 0.3, 0.0, 1.0));
    #endif
    #ifdef EXPERIMENTAL_GRADIENTSOBEL
        vec3 s2[8];
        s2[0] = aa_sample_grad(uv, vec2(-1,  1));
        s2[1] = aa_sample_grad(uv, vec2( 0,  1));
        s2[2] = aa_sample_grad(uv, vec2( 1,  1));
        s2[3] = aa_sample_grad(uv, vec2(-1,  0));
        s2[4] = aa_sample_grad(uv, vec2( 1,  0));
        s2[5] = aa_sample_grad(uv, vec2(-1, -1));
        s2[6] = aa_sample_grad(uv, vec2( 0, -1));
        s2[7] = aa_sample_grad(uv, vec2( 1, -1));
        vec3 gx2 = s2[0] + s2[3] * 2.0 + s2[5] - s2[2] - s2[4] * 2 - s2[7];
        vec3 gy2 = s2[0] + s2[1] * 2.0 + s2[2] - s2[5] - s2[6] * 2 - s2[7];
        float mag2 = length(gx2) + length(gy2);
        aa_color.rgb = mix(vec3(0.0), aa_color.rgb * 0.8, clamp(1.0 - mag2 * 0.3, 0.0, 1.0));
    #endif

    // Bloom
    #ifdef BLOOM_FACTOR
        vec4 bloom = textureLod(sampler2D(t_src_bloom, s_src_color), sample_uv, 0);
        #if (BLOOM_UNIFORM_BLUR == false)
            // divide by 4.0 to account for adding blurred layers together
            bloom /= 4.0;
        #endif
        aa_color = mix(aa_color, bloom, BLOOM_FACTOR);
    #endif

    // Tonemapping
    float exposure_offset = 1.0;
    // Adding an in-code offset to gamma and exposure let us have more precise control over the game's look
    #ifdef EXPERIMENTAL_CINEMATIC
        float gamma_offset = 0.5;
    #else
        float gamma_offset = 0.3;
    #endif
    aa_color.rgb = vec3(1.0) - exp(-aa_color.rgb * (gamma_exposure.y + exposure_offset));
    // gamma correction
    aa_color.rgb = pow(aa_color.rgb, vec3(gamma_exposure.x + gamma_offset));
    
    #ifdef EXPERIMENTAL_COLORQUANTIZATION
        const int QUANT_STEPS = 10;
        vec3 quant_color = pow(aa_color.rgb, vec3(0.25)) * QUANT_STEPS;
        ivec2 internal_res = textureSize(sampler2D(t_src_depth, s_src_depth), 0);
        #ifdef EXPERIMENTAL_COLORDITHERING
            vec3 quant_step = vec3(
                dither(ivec2(uv * internal_res + 0), fract(quant_color.r)),
                dither(ivec2(uv * internal_res + 1), fract(quant_color.g)),
                dither(ivec2(uv * internal_res + 2), fract(quant_color.b))
            );
        #else
            vec3 quant_step = step(hash_two_3(uvec2(uv * internal_res)), fract(quant_color));
        #endif
        aa_color.rgb = pow(floor(quant_color + quant_step) * (1.0 / QUANT_STEPS), vec3(4));
    #endif

    vec4 final_color = aa_color * vec4(vec3(screen_fade), 1.0);

#if (FLUID_MODE == FLUID_MODE_LOW)
    if (medium.x == MEDIUM_WATER) {
        final_color *= vec4(0.2, 0.2, 0.8, 1.0);
    }
#endif

#ifndef EXPERIMENTAL_NODITHER
    // Add a small amount of very cheap dithering noise to remove banding from gradients
    // TODO: Consider dithering each color channel independently.
    // TODO: Consider varying dither over time.
    // TODO: Instead of 255, detect the colour resolution of the color attachment
    float noise = hash_two(uvec2(uv * screen_res.xy));
    #ifndef EXPERIMENTAL_NONSRGBDITHER
        #ifndef EXPERIMENTAL_TRIANGLENOISEDITHER
            noise = noise - 0.5;
        #else
            // TODO: there is something special we have to do to remove bias
            // on the bounds when using triangle distribution
            noise = 2.0 * norm2tri(noise) - 1.0;
        #endif
        final_color.rgb = srgb_to_linear(linear_to_srgb(final_color.rgb) + noise / 255.0);
    #else
        // NOTE: GPU will clamp value
        final_color.rgb = final_color.rgb - noise / 255.0;
    #endif
#endif

    #ifdef EXPERIMENTAL_NEWSPAPER
        float nz = hash_three(uvec3(uvec2(uv * screen_res.xy), tick.x * dot(fract(uv * 10) + 5, vec2(1)) * 0.2));
        nz = (nz > 0.5) ? (pow(nz * 2 - 1, 1.5) * 0.5 + 0.5) : (pow(nz * 2, 1/1.5) * 0.5);
        final_color.rgb = vec3(step(nz, length(final_color.rgb))) * vec3(1, 0.5, 0.3);
    #else
        #ifdef EXPERIMENTAL_COLORDITHERING
            #ifndef EXPERIMENTAL_COLORQUANTIZATION
                float d = dither(ivec2(uv * screen_res.xy), sqrt(length(final_color.rgb) * 0.25));
                final_color.rgb = vec3(d) * sqrt(normalize(final_color.rgb));
            #endif
        #endif
    #endif

    #ifdef EXPERIMENTAL_CINEMATIC
        final_color.rgb = hsv2rgb(rgb2hsv(final_color.rgb) * vec3(1, 1, 1.3) + vec3(-0.01, 0.05, 0));
    #endif

    tgt_color = vec4(final_color.rgb, 1);
#endif
}
