#version 330 core

#define FIGURE_SHADER

#include <constants.glsl>

#define LIGHTING_TYPE LIGHTING_TYPE_REFLECTION

#define LIGHTING_REFLECTION_KIND LIGHTING_REFLECTION_KIND_GLOSSY

#define LIGHTING_TRANSPORT_MODE LIGHTING_TRANSPORT_MODE_IMPORTANCE

#define LIGHTING_DISTRIBUTION_SCHEME LIGHTING_DISTRIBUTION_SCHEME_MICROFACET

#define LIGHTING_DISTRIBUTION LIGHTING_DISTRIBUTION_BECKMANN

#include <globals.glsl>

in vec3 f_pos;
in vec3 f_col;
flat in vec3 f_norm;
in float f_ao;

layout (std140)
uniform u_locals {
    mat4 model_mat;
    vec4 highlight_col;
    vec4 model_light;
    vec4 model_glow;
    ivec4 atlas_offs;
    vec3 model_pos;
    int flags;
};

struct BoneData {
    mat4 bone_mat;
    mat4 normals_mat;
};

layout (std140)
uniform u_bones {
    BoneData bones[16];
};

#include <sky.glsl>
#include <light.glsl>
#include <srgb.glsl>

out vec4 tgt_color;

void main() {
    tgt_color = vec4(0.0, 0.0, 0.0, 1.0);
}
