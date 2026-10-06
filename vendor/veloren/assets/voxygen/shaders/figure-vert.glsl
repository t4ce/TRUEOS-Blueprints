#version 440 core

#include <constants.glsl>

#define FIGURE_SHADER

#define LIGHTING_TYPE LIGHTING_TYPE_REFLECTION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_GLOSSY

#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#include <globals.glsl>
#include <lod.glsl>

layout(location = 0) in uint v_pos_norm;
layout(location = 1) in uint v_atlas_pos;

layout (std140, set = 3, binding = 0)
uniform u_locals {
    mat4 model_mat;
    vec4 highlight_col;
    vec4 model_light;
    vec4 model_glow;
    ivec4 atlas_offs;
    vec3 model_pos;
    // bit 0 - is player
    // bit 1-31 - unused
    int flags;
};

struct BoneData {
    mat4 bone_mat;
    // This is actually a matrix, but we explicitly rely on being able to index into it
    // in column major order, and some shader compilers seem to transpose the matrix to
    // a different format when it's copied out of the array.  So we shouldn't put it in
    // a local variable (I think explicitly marking it as a vec4[4] works, but I'm not
    // sure whether it optimizes the same, and in any case the fact that there's a
    // format change suggests an actual wasteful copy is happening).
    mat4 normals_mat;
};

layout (std140, set = 3, binding = 1)
uniform u_bones {
    // Warning: might not actually be 16 elements long. Don't index out of bounds!
    BoneData bones[16];
};

layout(location = 0) out vec3 f_pos;
layout(location = 1) flat out vec3 f_norm;
layout(location = 2) out vec2 f_uv_pos;
layout(location = 3) out vec3 m_pos;
layout(location = 4) out float scale;

void main() {
    // Pre-calculate bone matrix
    uint bone_idx = (v_pos_norm >> 27) & 0xFu;

    vec3 pos = (vec3((uvec3(v_pos_norm) >> uvec3(0, 9, 18)) & uvec3(0x1FFu)) - 256.0) / 2.0;

    m_pos = pos;
    scale = length(bones[bone_idx].bone_mat[0]);

    f_pos = (
        bones[bone_idx].bone_mat *
        vec4(pos, 1.0)
    ).xyz + (model_pos - focus_off.xyz);

    #ifdef EXPERIMENTAL_CURVEDWORLD
        f_pos.z -= pow(distance(f_pos.xy + focus_off.xy, focus_pos.xy + focus_off.xy) * 0.05, 2);
    #endif

    f_uv_pos = vec2((uvec2(v_atlas_pos) >> uvec2(2, 17)) & uvec2(0x7FFFu, 0x7FFFu));

    // First 3 normals are negative, next 3 are positive
    uint axis_idx = v_atlas_pos & 3u;

    vec3 norm = bones[bone_idx].normals_mat[axis_idx].xyz;

    // Calculate normal here rather than for each pixel in the fragment shader
    f_norm = mix(-norm, norm, v_pos_norm >> 31u);

    gl_Position = all_mat * vec4(f_pos, 1);
}
