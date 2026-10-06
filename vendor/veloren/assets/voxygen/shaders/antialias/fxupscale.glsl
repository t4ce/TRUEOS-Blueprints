#include <fxaa.glsl>

vec4 aa_apply(
    texture2D tex, sampler smplr,
    texture2D depth_tex, sampler depth_smplr,
    vec2 fragCoord,
    vec2 resolution
) {
    ivec2 dirs[4] = { ivec2(-1, 0), ivec2(1, 0), ivec2(0, -1), ivec2(0, 1) };

    vec2 sz = textureSize(sampler2D(tex, smplr), 0).xy;

    float min_depth = 1000;
    float max_depth = 0;
    for (uint i = 0u; i < dirs.length(); i ++) {
        float d = texelFetch(sampler2D(depth_tex, depth_smplr), ivec2(fragCoord / screen_res.xy * sz) + dirs[i], 0).x;
        min_depth = min(min_depth, d);
        max_depth = max(max_depth, d);
    }

    vec4 aa_color = fxaa_apply(tex, smplr, fragCoord, resolution, 1.0 + 1.0 / (min_depth * 0 + 0.001 + (max_depth - min_depth) * 500) * 0.001);
    vec4 lerped = texture(sampler2D(tex, smplr), fragCoord / screen_res.xy);

    vec4 closest = aa_color;
    float closest_dist = 1000.0;
    for (uint i = 0u; i < dirs.length(); i ++) {
        vec4 col_at = texelFetch(sampler2D(tex, smplr), ivec2(fragCoord / screen_res.xy * sz) + dirs[i], 0);
        float dist = dot(pow(aa_color.rgb - col_at.rgb, ivec3(2)), vec3(1));
        if (dist < closest_dist) {
            closest = mix(col_at, lerped, min(length(lerped.rgb - col_at.rgb) * 0.25, 1));
            closest_dist = dist;
        }
    }
    return closest;
}
