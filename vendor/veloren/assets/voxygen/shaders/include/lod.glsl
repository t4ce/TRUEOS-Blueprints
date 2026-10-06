#ifndef LOD_GLSL
#define LOD_GLSL

#include <random.glsl>
#include <sky.glsl>
#include <srgb.glsl>

layout(set = 0, binding = 7) uniform texture2D t_horizon;
layout(set = 0, binding = 8) uniform sampler s_horizon;


const float MIN_SHADOW = 0.33;

vec2 pos_to_tex(vec2 pos) {
    // Want: (pixel + 0.5)
    vec2 uv_pos = (focus_off.xy + pos + 16) / 32.0;
    return vec2(uv_pos.x, uv_pos.y);
}

// textureBicubic from https://stackoverflow.com/a/42179924
vec4 cubic(float v) {
    vec4 n = vec4(1.0, 2.0, 3.0, 4.0) - v;
    vec4 s = n * n * n;
    float x = s.x;
    float y = s.y - 4.0 * s.x;
    float z = s.z - 4.0 * s.y + 6.0 * s.x;
    float w = 6.0 - x - y - z;
    return vec4(x, y, z, w) * (1.0/6.0);
}

// Computes atan(y, x), except with more stability when x is near 0.
float atan2(in float y, in float x) {
    bool s = (abs(x) > abs(y));
    return mix(PI/2.0 - atan(x,y), atan(y,x), s);
}

// NOTE: We assume the sampled coordinates are already in "texture pixels".
vec4 textureBicubic(texture2D tex, sampler sampl, vec2 texCoords) {
    // TODO: remove all textureSize calls and replace with constants
   vec2 texSize = textureSize(sampler2D(tex, sampl), 0);
   vec2 invTexSize = 1.0 / texSize;

   texCoords = texCoords/* * texSize */ - 0.5;


    vec2 fxy = fract(texCoords);
    texCoords -= fxy;

    vec4 xcubic = cubic(fxy.x);
    vec4 ycubic = cubic(fxy.y);

    vec4 c = texCoords.xxyy + vec2 (-0.5, +1.5).xyxy;

    vec4 s = vec4(xcubic.xz + xcubic.yw, ycubic.xz + ycubic.yw);
    vec4 offset = c + vec4 (xcubic.yw, ycubic.yw) / s;

    offset *= invTexSize.xxyy;

    vec4 sample0 = texture(sampler2D(tex, sampl), offset.xz);
    vec4 sample1 = texture(sampler2D(tex, sampl), offset.yz);
    vec4 sample2 = texture(sampler2D(tex, sampl), offset.xw);
    vec4 sample3 = texture(sampler2D(tex, sampl), offset.yw);

    float sx = s.x / (s.x + s.y);
    float sy = s.z / (s.z + s.w);

    return mix(
       mix(sample3, sample2, sx), mix(sample1, sample0, sx)
    , sy);
}

vec4 textureMaybeBicubic(texture2D tex, sampler sampl, vec2 texCoords) {
    // TODO: Allow regular `texture` to be used when cause of light leaking issues is found
    //#if (CLOUD_MODE >= CLOUD_MODE_HIGH)
        return textureBicubic(tex, sampl, texCoords);
    //#else
    //    vec2 offset = (texCoords + vec2(-1.0, 0.5)) / textureSize(sampler2D(tex, sampl), 0);
    //    return texture(sampler2D(tex, sampl), offset);
    //#endif
}

// 16 bit version (each of the 2 8-bit components are combined after bilinear sampling)
// NOTE: We assume the sampled coordinates are already in "texture pixels".
vec2 textureBicubic16(texture2D tex, sampler sampl, vec2 texCoords) {
   vec2 texSize = textureSize(sampler2D(tex, sampl), 0);
   vec2 invTexSize = 1.0 / texSize;

   texCoords = texCoords - 0.5;


    vec2 fxy = fract(texCoords);
    texCoords -= fxy;

    vec4 xcubic = cubic(fxy.x);
    vec4 ycubic = cubic(fxy.y);

    vec4 c = texCoords.xxyy + vec2 (-0.5, +1.5).xyxy;

    vec4 s = vec4(xcubic.xz + xcubic.yw, ycubic.xz + ycubic.yw);
    vec4 offset = c + vec4 (xcubic.yw, ycubic.yw) / s;

    offset *= invTexSize.xxyy;

    vec4 sample0_v4 = textureLod(sampler2D(tex, sampl), offset.xz, 0);
    vec4 sample1_v4 = textureLod(sampler2D(tex, sampl), offset.yz, 0);
    vec4 sample2_v4 = textureLod(sampler2D(tex, sampl), offset.xw, 0);
    vec4 sample3_v4 = textureLod(sampler2D(tex, sampl), offset.yw, 0);
    vec2 sample0 = sample0_v4.rb / 256.0 + sample0_v4.ga;
    vec2 sample1 = sample1_v4.rb / 256.0 + sample1_v4.ga;
    vec2 sample2 = sample2_v4.rb / 256.0 + sample2_v4.ga;
    vec2 sample3 = sample3_v4.rb / 256.0 + sample3_v4.ga;

    float sx = s.x / (s.x + s.y);
    float sy = s.z / (s.z + s.w);

    return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
}

// Gets the altitude at a position relative to focus_off.
float alt_at(vec2 pos) {
    vec4 alt_sample = textureLod(sampler2D(t_alt, s_alt), wpos_to_uv(focus_off.xy + pos), 0);
    return (((alt_sample.r * (1.0 / 256.0) + alt_sample.g) * view_distance.w) + view_distance.z - focus_off.z);
}

float alt_at_real(vec2 pos) {
    return ((textureBicubic16(t_alt, s_alt, pos_to_tex(pos)).r * view_distance.w) + view_distance.z - focus_off.z);
}


float horizon_at2(vec4 f_horizons, float alt, vec3 pos, vec4 light_dir) {
    const float PI_2 = 3.1415926535897932384626433832795 / 2.0;
    const float MIN_LIGHT = 0.0;
    
    vec2 f_horizon = mix(f_horizons.rg, f_horizons.ba, bvec2(light_dir.x < 0.0));
    float angle = tan(f_horizon.x * PI_2);
    float height = f_horizon.y * view_distance.w + view_distance.z;
    const float w = 0.1;
    float deltah = height - alt - focus_off.z;
    float lighta = -light_dir.z / max(abs(light_dir.x), 0.0001);
    // NOTE: Ideally, deltah <= 0.0 is a sign we have an oblique horizon angle.
    float deltax = deltah / max(angle, 0.0001);
    float lighty = lighta * deltax;
    float deltay = lighty - deltah + max(pos.z - alt, 0.0);
    // NOTE: the "real" deltah should always be >= 0, so we know we're only handling the 0 case with max.
    float s = mix(max(min(max(deltay, 0.0) / max(deltax, 0.0001) / w, 1.0), 0.0), 1.0, deltah <= 0);
    return max(s * s * (3.0 - 2.0 * s), MIN_LIGHT);
}

vec2 splay(vec2 pos) {
    vec2 scale = textureSize(sampler2D(t_alt, s_alt), 0) * 32.0;
    float lod_dist = view_distance.x * 0.95 / max(scale.x, scale.y);
    float dist = abs(pos.x) + abs(pos.y);
    float stretch = (pow(dist, 5.5) * 0.75 + dist * 0.25) * (1.0 - lod_dist) + lod_dist;
    vec2 splayed = pos * stretch * scale;
    if (abs(pos.x) > 0.99 || abs(pos.y) > 0.99) {
        splayed *= 50.0;
    }
    return splayed;
}

vec3 lod_norm(vec2 f_pos/*vec3 pos*/, vec4 square) {
    float altx0 = alt_at(vec2(square.x, f_pos.y));
    float altx1 = alt_at(vec2(square.z, f_pos.y));
    float alty0 = alt_at(vec2(f_pos.x, square.y));
    float alty1 = alt_at(vec2(f_pos.x, square.w));
    float slope = abs(altx1 - altx0) + abs(alty0 - alty1);

    vec3 norm = normalize(vec3(
        (altx0 - altx1) / (square.z - square.x),
        (alty0 - alty1) / (square.w - square.y),
        1.0
    ));

    return faceforward(norm, vec3(0.0, 0.0, -1.0), norm);
}

vec3 lod_norm(vec2 f_pos) {
    const float SAMPLE_W = 32;
    return lod_norm(f_pos, vec4(f_pos - vec2(SAMPLE_W), f_pos + vec2(SAMPLE_W)));
}


vec3 lod_pos(vec2 pos, vec2 focus_pos) {
    // Remove spiking by "pushing" vertices towards local optima
    vec2 delta = splay(pos);
    vec2 hpos = focus_pos + delta;

    vec2 dir = normalize(pos);
    float shift = 150.0 * pow(length(pos), 3.0);
    for (int i = 1; i < 10; i ++) {
        hpos -= dir * dot(normalize(lod_norm(hpos)).xy, dir) * shift / float(i);
    }

    return vec3(hpos, alt_at_real(hpos));
}

#ifdef HAS_LOD_FULL_INFO
layout(set = 0, binding = 10)
uniform texture2D t_map;
layout(set = 0, binding = 11)
uniform sampler s_map;

vec3 lod_col(vec2 pos) {
    #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
        vec2 wpos = pos + focus_off.xy;
        vec2 shift = vec2(
            textureLod(sampler2D(t_noise, s_noise), wpos / 200, 0).x - 0.5,
            textureLod(sampler2D(t_noise, s_noise), wpos / 200 + 0.5, 0).x - 0.5
        ) * 32 + vec2(
            textureLod(sampler2D(t_noise, s_noise), wpos / 50, 0).x - 0.5,
            textureLod(sampler2D(t_noise, s_noise), wpos / 50 + 0.5, 0).x - 0.5
        ) * 16;
        pos += shift;
        wpos += shift;
    #endif

    vec3 col = textureBicubic(t_map, s_map, pos_to_tex(pos)).rgb;

    return col;
}
#endif

vec3 water_diffuse(vec3 color, vec3 dir, float max_dist) {
    if (medium.x == 1) {
        float f_alt = alt_at(cam_pos.xy);
        float fluid_alt = max(cam_pos.z + 1, floor(f_alt + 1));

        float water_dist = clamp((fluid_alt - cam_pos.z) / pow(max(dir.z, 0), 2), 0, max_dist);

        float fade = pow(0.95, water_dist);

        return mix(vec3(0.0, 0.2, 0.5)
            * (get_sun_brightness() * get_sun_color() + get_moon_brightness() * get_moon_color())
            * pow(0.99, max((fluid_alt - cam_pos.z) * 12.0 - dir.z * 200, 0)), color.rgb * exp(-MU_WATER * water_dist * 0.1), fade);
    } else {
        return color;
    }
}

void lod_voxels(vec3 f_pos, vec3 f_norm, vec3 cam_dir, out vec3 voxel_pos, out vec3 voxel_norm, out float voxel_sz, out float f_ao) {
    voxel_pos = f_pos;
    voxel_norm = f_norm;
    voxel_sz = 1.0;
    f_ao = 1.0;
    
    #ifndef EXPERIMENTAL_NOLODVOXELS
        const float VOXEL_SCALE_FACTOR = 100000.0;
        vec3 wpos = f_pos + focus_off.xyz;
        
        voxel_sz = clamp(exp(floor(log(distance(cam_pos.xy, f_pos.xy) * 0.0001 + noise_2d(wpos.xy * 0.01) * 0.02) * 3) / 3) * VOXEL_SCALE_FACTOR / (internal_res.x + internal_res.y), 1.0, 128.0);
        
        #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
            const float MARCH_THRESHOLD = 4.0;
        #else
            const float MARCH_THRESHOLD = 2.0;
        #endif
        
        float t = -MARCH_THRESHOLD * voxel_sz;
        int i = 0;
        while (t < MARCH_THRESHOLD * voxel_sz && i++<40) {
            vec3 deltas = (fract((wpos + cam_dir * t) / voxel_sz) - step(vec3(0), cam_dir * voxel_sz)) / -cam_dir * voxel_sz;
            t += max(min(min(deltas.x, deltas.y), deltas.z), 0.001);

            voxel_pos = (floor((wpos + cam_dir * t) / voxel_sz) + 0.5) * voxel_sz;
            float surf_depth = 0.0;
            #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
                surf_depth = (noise_3d(voxel_pos / voxel_sz * 0.01) - 0.5)
                    * 10.0
                    * voxel_sz
                    * pow(mix(0.0, mix(1.0, 0.0, max(f_norm.z, 0.0)), max(f_norm.z, 0.0)), 0.5);
            #endif
            if (dot(voxel_pos - wpos, -f_norm) > surf_depth) {
                vec3 to_center = abs(voxel_pos - (wpos + cam_dir * t));
                voxel_norm = step(max(max(to_center.x, to_center.y), to_center.z), to_center) * sign(-cam_dir);
                float dist = dot(cam_dir * t, f_norm) + surf_depth;
                f_ao = clamp(dist / voxel_sz + max(f_norm.z, 0.5), 0.25, 1.0);
                voxel_pos -= focus_off.xyz;
                return;
            }
        }
        voxel_pos = f_pos;
        // Fallback, if we didn't hit any voxels
        voxel_norm = step(max(max(f_norm.x, f_norm.y), f_norm.z), f_norm) * sign(-cam_dir);
    #endif
}

#endif
