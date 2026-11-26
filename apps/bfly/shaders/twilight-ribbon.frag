#version 450 core

#ifdef VULKAN
layout(set = 1, binding = 0) uniform sampler2D twilight_density;
#else
layout(binding = 0) uniform sampler2D twilight_density;
#endif
layout(location = 0) in vec4 ribbon;
layout(location = 0) out vec4 color;

void main() {
    float density = texture(twilight_density, fract(ribbon.xy)).r;
    // Most texels in the starfield are dark. Reserve the intense amber for
    // the bright nebula cores and stars while keeping its ink-like gaps.
    float nebula = smoothstep(0.035, 0.30, density);
    float sparks = smoothstep(0.30, 0.68, density);
    vec3 ink = vec3(0.029, 0.018, 0.071);
    vec3 red = vec3(0.85, 0.055, 0.08);
    vec3 amber = vec3(1.0, 0.61, 0.25);
    vec3 tint = mix(mix(ink, red, nebula), amber, sparks);
    float decay = 1.0 - ribbon.z;
    float fade = smoothstep(0.0, 0.055, ribbon.z) * decay * decay;
    float opacity = ribbon.w * fade * mix(0.56, 1.0, nebula);
    color = vec4(tint * opacity, opacity);
}
