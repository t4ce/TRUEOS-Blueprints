#version 450 core

layout(location = 0) in vec2 control_point;
layout(location = 1) in float straight_edge;
layout(location = 0) out float patch_straight;

void main() {
#ifdef VULKAN
    int instance = gl_InstanceIndex;
#else
    int instance = gl_InstanceID;
#endif
    float mirror_x = ((instance & 1) == 0) ? 1.0 : -1.0;
    // Two sides share a depth. Each successive butterfly is twice as far away.
    float view_z = -float(1u << uint(instance >> 1));
    gl_Position = vec4(control_point.x * mirror_x, control_point.y, view_z, 1.0);
    patch_straight = straight_edge;
}
