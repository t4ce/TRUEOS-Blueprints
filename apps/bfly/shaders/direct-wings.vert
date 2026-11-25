#version 450 core

#if defined(BFLY_TIER_WIDTH) && !defined(VULKAN)
#extension GL_ARB_shader_draw_parameters : require
#endif

layout(location = 0) in vec3 control_point_and_straight_edge;
layout(location = 0) flat out int panel_side;

void main() {
#ifdef VULKAN
    panel_side = gl_InstanceIndex;
#elif defined(BFLY_TIER_WIDTH)
    panel_side = gl_InstanceID + gl_BaseInstanceARB;
#else
    panel_side = gl_InstanceID;
#endif
    gl_Position = vec4(control_point_and_straight_edge, 1.0);
}
