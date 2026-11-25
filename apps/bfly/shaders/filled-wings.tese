#version 450 core

layout(quads, equal_spacing, ccw) in;
layout(location = 1) patch in int straight_edge;
layout(location = 2) patch in vec3 clip_from_x;
layout(location = 3) patch in vec3 clip_from_y;
layout(location = 4) patch in vec3 clip_from_origin;
#ifdef BFLY_BREED_FILL
layout(location = 0) flat out uint fill_rgb;
#endif

#ifdef BFLY_INSET_FILL
// Both closed contours are visible from their respective hinge points.
// Scaling about the hinge keeps the mirrored fills joined.
const vec2 HINGE_ORIGINS[2] = vec2[2](vec2(0.0, 0.011652004718780529),
                                     vec2(0.0, -0.0044550925493240245));
const float INSET_SCALE = sqrt(0.8);
#else
// One interior point per closed lobe. The authored curves and the closing
// chord are visible from the point, including the concave original top lobe.
const vec2 LOBE_ORIGINS[4] = vec2[4](
    vec2(0.377107, 0.150644), vec2(0.253212, -0.161357),
    vec2(0.376804, 0.089086), vec2(0.237782, -0.194439)
);
#endif

void main() {
    float t = gl_TessCoord.x;
    vec2 p0 = gl_in[0].gl_Position.xy;
    vec2 p3 = gl_in[3].gl_Position.xy;
    vec2 boundary;
    if ((straight_edge & 1) != 0) {
        boundary = mix(p0, p3, t);
    } else {
        vec2 p1 = gl_in[1].gl_Position.xy;
        vec2 p2 = gl_in[2].gl_Position.xy;
        vec2 a = mix(p0, p1, t);
        vec2 b = mix(p1, p2, t);
        vec2 c = mix(p2, p3, t);
        boundary = mix(mix(a, b, t), mix(b, c, t), t);
    }
#ifdef BFLY_INSET_FILL
#ifdef BFLY_BREED_FILL
    int artwork = straight_edge >> 2;
    artwork &= 1;
    fill_rgb = uint(straight_edge) >> 3u;
    vec2 point = mix(HINGE_ORIGINS[artwork], boundary, INSET_SCALE * gl_TessCoord.y);
#else
    vec2 point = mix(HINGE_ORIGINS[straight_edge >> 2], boundary, INSET_SCALE * gl_TessCoord.y);
#endif
#else
    vec2 point = mix(LOBE_ORIGINS[straight_edge >> 1], boundary, gl_TessCoord.y);
#endif
    vec3 clip = clip_from_x * point.x + clip_from_y * point.y + clip_from_origin;
    gl_Position = vec4(clip.xy, 0.5 * clip.z, clip.z);
}
