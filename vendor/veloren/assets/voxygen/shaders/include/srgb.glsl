#ifndef SRGB_GLSL
#define SRGB_GLSL

#extension GL_EXT_samplerless_texture_functions : enable

// Linear RGB, attenuation coefficients for water at roughly R, G, B wavelengths.
// See https://en.wikipedia.org/wiki/Electromagnetic_absorption_by_water
const vec3 MU_WATER = vec3(0.6, 0.04, 0.01);

//https://gamedev.stackexchange.com/questions/92015/optimized-linear-to-srgb-glsl
vec3 srgb_to_linear(vec3 srgb) {
    bvec3 cutoff = lessThan(srgb, vec3(0.04045));
    vec3 higher = pow((srgb + vec3(0.055))/vec3(1.055), vec3(2.4));
    vec3 lower = srgb/vec3(12.92);

    return mix(higher, lower, cutoff);
}

vec3 linear_to_srgb(vec3 col) {
    vec3 s1 = vec3(sqrt(col.r), sqrt(col.g), sqrt(col.b));
    vec3 s2 = vec3(sqrt(s1.r), sqrt(s1.g), sqrt(s1.b));
    vec3 s3 = vec3(sqrt(s2.r), sqrt(s2.g), sqrt(s2.b));
    return vec3(
            mix(11.500726 * col.r, (0.585122381 * s1.r + 0.783140355 * s2.r - 0.368262736 * s3.r), clamp((col.r - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.g, (0.585122381 * s1.g + 0.783140355 * s2.g - 0.368262736 * s3.g), clamp((col.g - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.b, (0.585122381 * s1.b + 0.783140355 * s2.b - 0.368262736 * s3.b), clamp((col.b - 0.0060) * 10000.0, 0.0, 1.0))
    );
}

float pow5(float x) {
    float x2 = x * x;
    return x2 * x2 * x;
}

vec4 pow5(vec4 x) {
    vec4 x2 = x * x;
    return x2 * x2 * x;
}

// Fresnel angle for perfectly specular dialectric materials.

// Schlick approximation
vec3 schlick_fresnel(vec3 Rs, float cosTheta) {
    return Rs + pow5(1.0 - cosTheta) * (1.0 - Rs);
}

// Beckmann Distribution
float BeckmannDistribution_D(float NdotH, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    float NdotH2 = NdotH * NdotH;
    float NdotH2m2 = NdotH2 * alpha * alpha;
    float k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return mix(k_spec, 0.0, NdotH == 0.0);
}

// Voxel Distribution
float BeckmannDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);
    
    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float TrowbridgeReitzDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);

    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 e = (1.0 - NdotH2) / NdotH2m2;
    vec3 k_spec = 1.0 / (PI * NdotH2m2 * NdotH2 * (1.0 + e) * (1.0 + e));
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float BeckmannDistribution_Lambda(vec3 norm, vec3 dir, float alpha) {
    float CosTheta = dot(norm, dir);
    float SinTheta = sqrt(1.0 - CosTheta * CosTheta);
    float TanTheta = SinTheta / CosTheta;
    float absTanTheta = abs(TanTheta);
    float a = 1.0 / (alpha * absTanTheta);
    
    return mix(max(0.0, (1.0 - 1.259 * a + 0.396 * a * a) / (3.535 * a + 2.181 * a * a)), 0.0, isinf(absTanTheta) || a >= 1.6);
}

float BeckmannDistribution_G(vec3 norm, vec3 dir, vec3 light_dir, float alpha) {
    return 1.0 / (1.0 + BeckmannDistribution_Lambda(norm, dir, alpha) + BeckmannDistribution_Lambda(norm, -light_dir, alpha));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d *
        (1.0 - R_s) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wi))) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wo)));
    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    vec3 specular = dot(norm, dir) > 0.0 ? vec3(0.0) : (BeckmannDistribution_D(dot(wh, norm), alpha) /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh));
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_Voxel_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha, vec3 voxel_norm, float dist) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, cos_wi));
    vec4 AbsNdotV = abs(vec4(dir, cos_wo));
#else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = vec4(max(-light_dir * sides, 0.0), abs(cos_wi));
    vec4 AbsNdotV = vec4(max(dir * sides, 0.0), abs(cos_wo));
#endif

    vec4 diffuse_factor = (1.0 - pow5(1.0 - 0.5 * AbsNdotL)) * (1.0 - pow5(1.0 - 0.5 * AbsNdotV));

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d * (1.0 - R_s) * dot(diffuse_factor, /*R_r * */vec4(abs(norm) * (1.0 - dist), dist));

    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    float distr = BeckmannDistribution_D_Voxel(wh, voxel_norm, alpha);
    vec3 specular = distr /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh);
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Phong reflection.
//
// Note: norm, dir, light_dir must all be normalizd.
vec3 light_reflection_factor2(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha) {
    // TODO: These are supposed to be the differential changes in the point location p, in tangent space.
    // That is, assuming we can parameterize a 2D surface by some function p : R² → R³, mapping from
    // points in a plane to 3D points on the surface, we can define
    // ∂p(u,v)/∂u and ∂p(u,v)/∂v representing the changes in the pont location as we move along these
    // coordinates.
    //
    // Then we can define the normal at a point, n(u,v) = ∂p(u,v)/∂u × ∂p(u,v)/∂v.
    //
    // Additionally, we can define the change in *normals* at each point using the
    // Weingarten equations (see http://www.pbr-book.org/3ed-2018/Shapes/Spheres.html):
    //
    // ∂n/∂u = (fF - eG) / (EG - F²) ∂p/∂u + (eF - fE) / (EG - F²) ∂p/∂v
    // ∂n/∂v = (gF - fG) / (EG - F²) ∂p/∂u + (fF - gE) / (EG - F²) ∂p/∂v
    //
    // where
    //
    // E = |∂p/∂u ⋅ ∂p/∂u|
    // F = ∂p/∂u ⋅ ∂p/∂u
    // G = |∂p/∂v ⋅ ∂p/∂v|
    //
    // and
    //
    // e = n ⋅ ∂²p/∂u²
    // f = n ⋅ ∂²p/(∂u∂v)
    // g = n ⋅ ∂²p/∂v²
    //
    // For planes (see http://www.pbr-book.org/3ed-2018/Shapes/Triangle_Meshes.html) we have
    // e = f = g = 0 (since the plane has no curvature of any sort) so we get:
    //
    // ∂n/∂u = (0, 0, 0)
    // ∂n/∂v = (0, 0, 0)
    //
    // To find ∂p/∂u and ∂p/∂v, we first write p and u parametrically:
    //    p(u, v) = p0 + u ∂p/∂u + v ∂p/∂v
    //
    // ( u₀ - u₂    v₀ - v₂
    //   u₁ - u₂    v₁ - v₂ )
    //
    // Basis: plane norm = norm = (0, 0, 1), x vector = any orthgonal vector on the plane.
    // vec3 w_i =
    // vec3 w_i = vec3(view_mat * vec4(-light_dir, 1.0));
    // vec3 w_o = vec3(view_mat * vec4(light_dir, 1.0));
    return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
}

vec3 light_reflection_factor(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha, vec3 voxel_norm, float voxel_lighting) {
#if (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_LAMBERTIAN)
    const float PI = 3.141592;
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, dot(norm, light_dir)));
        #else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = max(vec4(-light_dir * sides, dot(norm, -light_dir)), 0.0);
        #endif
    float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float diffuse = abs(dot(norm, light_dir));
        #else
    float diffuse = max(dot(norm, -light_dir), 0.0);
        #endif
    #endif
    return k_d / PI * diffuse;
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_BLINN_PHONG)
    const float PI = 3.141592;
    alpha = alpha * sqrt(2.0);
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float ndotL = abs(dot(norm, light_dir));
    #else
    float ndotL = max(dot(norm, -light_dir), 0.0);
    #endif

    if (ndotL > 0.0) {
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        vec4 AbsNdotL = abs(vec4(light_dir, ndotL));
        #else
        vec3 sides = sign(voxel_norm);
        vec4 AbsNdotL = max(vec4(-light_dir * sides, ndotL), 0.0);
        #endif
        float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        float diffuse = ndotL;
    #endif
        vec3 H = normalize(-light_dir + dir);

    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        float NdotH = abs(dot(norm, H));
    #else
        float NdotH = max(dot(norm, H), 0.0);
    #endif
        return (1.0 - k_s) / PI * k_d * diffuse + k_s * pow(NdotH, alpha/* * 4.0*/);
    }

    return vec3(0.0);
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_ASHIKHMIN)
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        return FresnelBlend_Voxel_f(norm, dir, light_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting);
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
    #endif
#endif
}

float rel_luminance(vec3 rgb)
{
    // https://en.wikipedia.org/wiki/Relative_luminance
    const vec3 W = vec3(0.2126, 0.7152, 0.0722);
    return dot(rgb, W);
}

// From https://discourse.vvvv.org/t/infinite-ray-intersects-with-infinite-plane/10537
// out of laziness.
bool IntersectRayPlane(vec3 rayOrigin, vec3 rayDirection, vec3 posOnPlane, vec3 planeNormal, inout vec3 intersectionPoint)
{
  float rDotn = dot(rayDirection, planeNormal);

  //parallel to plane or pointing away from plane?
  if (rDotn < 0.0000001 )
    return false;

  float s = dot(planeNormal, (posOnPlane - rayOrigin)) / rDotn;

  intersectionPoint = rayOrigin + s * rayDirection;

  return true;
}

// Compute uniform attenuation due to beam passing through a substance that fills an area below a horizontal plane
// (e.g. in most cases, water below the water surface depth) using the simplest form of the Beer-Lambert law
// (https://en.wikipedia.org/wiki/Beer%E2%80%93Lambert_law):
//
// I(z) = I₀ e^(-μz)
//
// We compute this value, except for the initial intensity which may be multiplied out later.
//
// wpos is the position of the point being hit.
// ray_dir is the reversed direction of the ray (going "out" of the point being hit).
// mu is the attenuation coefficient for R, G, and B wavelenghts.
// surface_alt is the estimated altitude of the horizontal surface separating the substance from air.
// defaultpos is the position to use in computing the distance along material at this point if there was a failure.
//
// Ideally, defaultpos is set so we can avoid branching on error.
vec3 compute_attenuation(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return vec3(1.0);
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        return vec3(1.0);
    #else
    ray_dir = faceforward(ray_dir, vec3(0.0, 0.0, -1.0), ray_dir);
    vec3 surface_dir = surface_alt < wpos.z ? vec3(0.0, 0.0, -1.0) : vec3(0.0, 0.0, 1.0);
    bool _intersects_surface = IntersectRayPlane(wpos, ray_dir, vec3(0.0, 0.0, surface_alt), surface_dir, defaultpos);
    float depth = length(defaultpos - wpos);
    return exp(-mu * depth);
    #endif
#endif
}

// Same as compute_attenuation but since both point are known, set a maximum to make sure we don't exceed the length
// from the default point.
vec3 compute_attenuation_point(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return pow(1.0 - mu, vec3(3));
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    return vec3(1.0);
#endif
}

vec3 greedy_extract_col_light_attr(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out float f_glow, out float f_ao, out uint f_attr, out float f_sky_exposure) {
    // TODO: Figure out how to use `texture` and modulation to avoid needing to do manual filtering
    // TODO: Use `texture` instead

    uvec4 tex_00 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 0), 0) * 255.0);
    uvec4 tex_10 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 0), 0) * 255.0);
    uvec4 tex_01 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 1), 0) * 255.0);
    uvec4 tex_11 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 1), 0) * 255.0);
    vec3 light_00 = vec3(tex_00.rg >> 3u, tex_00.a & 1u);
    vec3 light_10 = vec3(tex_10.rg >> 3u, tex_10.a & 1u);
    vec3 light_01 = vec3(tex_01.rg >> 3u, tex_01.a & 1u);
    vec3 light_11 = vec3(tex_11.rg >> 3u, tex_11.a & 1u);
    vec3 light_0 = mix(light_00, light_01, fract(f_uv_pos.y));
    vec3 light_1 = mix(light_10, light_11, fract(f_uv_pos.y));
    vec3 light = mix(light_0, light_1, fract(f_uv_pos.x));

    vec3 f_col = vec3(
        float(((tex_00.r & 0x7u) << 1u) | (tex_00.b & 0xF0u)),
        float(tex_00.a & 0xFEu),
        float(((tex_00.g & 0x7u) << 1u) | ((tex_00.b & 0x0Fu) << 4u))
    ) / 255.0;

    f_ao = light.z;
    f_light = light.x / 31.0;
    f_sky_exposure = light.x / 31.0 + (1.0 - f_ao) * 0.5;
    f_glow = light.y / 31.0;
    f_attr = tex_00.g >> 3u;
    return srgb_to_linear(f_col);
}

vec3 greedy_extract_col_light_kind_terrain(
    texture2D t_col_light, sampler s_col_light,
    utexture2D t_kind,
    vec2 f_uv_pos,
    out float f_light, out float f_glow, out float f_ao, out float f_sky_exposure, out uint f_kind
) {
    uint _f_attr;
    f_kind = uint(texelFetch(t_kind, ivec2(f_uv_pos), 0).r);
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, f_glow, f_ao, _f_attr, f_sky_exposure);
}

vec3 greedy_extract_col_light_figure(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out uint f_attr) {
    float _f_sky_exposure, _f_light, _f_glow, _f_ao;
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, _f_glow, _f_ao, f_attr, _f_sky_exposure);
}

#endif
