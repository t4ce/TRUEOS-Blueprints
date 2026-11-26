#version 450 core

#ifdef VULKAN
layout(set = 1, binding = 0) uniform sampler2D wake_atlas;
#else
layout(binding = 0) uniform sampler2D wake_atlas;
#endif
layout(location = 0) in vec4 ribbon;
layout(location = 1) in vec4 tear_and_pan;
layout(location = 2) flat in uint wake_weights;
layout(location = 0) out vec4 color;

vec2 atlas_tile(vec2 uv, float row) {
    const float half_texel = 0.5 / 256.0;
    return vec2(uv.x, (clamp(uv.y, half_texel, 1.0 - half_texel) + row) / 3.0);
}

float wake_alpha(float age, float angle, float mask_repeat, float drift, float amount, float falloff) {
    // Rotate the shared tear mask in local wing space.
    vec2 basis = mask_repeat * vec2(cos(angle), sin(angle));
    vec2 uv = tear_and_pan.xy + vec2(0.14 * drift * tear_and_pan.w, 0.0);
    uv = mat2(basis.x, -basis.y, basis.y, basis.x) * (uv - 0.5) + 0.5;
    vec2 tear_uv = 1.0 - abs(mod(uv, 2.0) - 1.0);
    float mask = texture(wake_atlas, atlas_tile(tear_uv, 0.0)).a;
    float erosion = smoothstep(0.06, 0.57, age) * amount;
    float cut = mix(-0.11, 0.74, erosion);
    float feather = clamp(fwidth(mask) * 0.75, 0.025, 0.16);
    float tear = smoothstep(cut - feather, cut + feather, mask);
    float fade = smoothstep(0.0, 0.055, age) * pow(1.0 - age, falloff);
    return ribbon.w * fade * tear;
}

vec3 sky_color(vec2 uv, float row) {
    return texture(wake_atlas, atlas_tile(fract(uv), row)).rgb;
}

void main() {
    float age = clamp(ribbon.z, 0.0, 1.0);
    uint sunrise = wake_weights & 255u;
    uint twilight = wake_weights >> 8u;
    uint universe = 255u - sunrise - twilight;
    vec2 pan = vec2(0.0, tear_and_pan.z);
    vec4 mixed_wake = vec4(0.0);

    const float original_angle = radians(176.3461538);
    if (sunrise > 0u) {
        float alpha = wake_alpha(age, original_angle, 4.05, 1.45, 0.74, 1.0);
        vec2 uv = vec2(0.18 + ribbon.x, 0.78 - 0.69 * age + 0.35 * ribbon.y);
        vec3 rgb = sky_color((uv - 0.5) * 1.35 + 0.5 - pan, 0.0);
        mixed_wake += (alpha * float(sunrise)) * vec4(rgb, 1.0);
    }
    if (twilight > 0u) {
        float alpha = wake_alpha(age, original_angle, 1.95, 1.4, 0.57, 0.9);
        vec3 rgb = sky_color(ribbon.xy * 2.0 + pan, 1.0);
        mixed_wake += (alpha * float(twilight)) * vec4(rgb, 1.0);
    }
    if (universe > 0u) {
        float alpha = wake_alpha(age, radians(-175.0), 3.3, 0.4, 0.21, 0.8);
        vec3 rgb = sky_color(ribbon.xy * 1.9 + pan, 2.0);
        mixed_wake += (alpha * float(universe)) * vec4(rgb, 1.0);
    }
    color = mixed_wake * (1.0 / 255.0);
}
