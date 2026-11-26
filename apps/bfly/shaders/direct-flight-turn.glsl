const float TURN_SAMPLE_SECONDS = 0.5;

// The wing mesh points along +Y. Compose a pitch around +X with a yaw
// around +Z so the sampled poses have the existing flight orientation.
vec4 flight_heading(vec3 tangent) {
    float horizontal = length(tangent.xy);
    if (horizontal < 1e-10) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    vec2 yaw = vec2(tangent.y, -tangent.x) / horizontal;
    vec2 z;
    if (yaw.x >= 0.0) {
        z.x = sqrt(0.5 * (1.0 + yaw.x));
        z.y = yaw.y / (2.0 * z.x);
    } else {
        z.y = (yaw.y < 0.0 ? -1.0 : 1.0) * sqrt(0.5 * (1.0 - yaw.x));
        z.x = yaw.y / (2.0 * z.y);
    }
    float speed = length(tangent);
    vec2 x;
    x.x = sqrt(0.5 * (1.0 + horizontal / speed));
    x.y = tangent.z / (2.0 * speed * x.x);
    return vec4(z * x.y, z.yx * x.x);
}

vec4 shortest_heading(vec4 from, vec4 to, float progress) {
    float cosine = dot(from, to);
    if (cosine < 0.0) {
        to = -to;
        cosine = -cosine;
    }
    if (cosine > 0.9995) {
        return normalize(mix(from, to, progress));
    }
    float arc = acos(cosine);
    return (sin((1.0 - progress) * arc) * from + sin(progress * arc) * to) / sin(arc);
}

mat3 heading_pose(vec4 heading) {
    vec3 twice = (2.0 / dot(heading, heading)) * heading.xyz;
    vec3 x = heading.x * twice;
    vec3 y = heading.y * twice;
    vec3 w = heading.w * twice;
    return mat3(1.0 - y.y - heading.z * twice.z, x.y + w.z, x.z - w.y,
                x.y - w.z, 1.0 - x.x - heading.z * twice.z, y.z + w.x,
                x.z + w.y, y.z - w.x, 1.0 - x.x - y.y);
}

vec4 heading_at(int coordinate, int seed, float speed_percent, float depth_phase,
                vec2 size, vec2 anchor, float spread, Placement placement, PanelSwap swap,
                float flight_time, float panel_time) {
    PanelSwap sample_swap = panel_swap(panel_time);
    if (sample_swap.flipped != swap.flipped || sample_swap.depth != swap.depth
        || sample_swap.blend != swap.blend || sample_swap.blend_rate != swap.blend_rate) {
        placement = tier_placement(coordinate, size, anchor, spread, sample_swap);
    }
    vec2 orbit, orbit_rate, depth_drift;
    flight_path(seed, speed_percent, max(framebuffer.w, 1.0), depth_phase, flight_time,
                orbit, orbit_rate, depth_drift);
    float depth_rate = LN_2 * depth_drift.y;
    // A settled tier has constant level and radius. Actual depth multiplies
    // all three tangent components equally, so the pose only needs this ratio.
    vec2 planar_rate = placement.center_rate
                      + (placement.center - 0.5 * size) * depth_rate
                      + placement.screen_radius * (orbit_rate + orbit * depth_rate);
    return flight_heading(vec3(planar_rate / FOCAL_LENGTH, -depth_rate));
}

mat3 flight_pose(int source, int target, int seed, float progress, float speed_percent,
                 float depth_phase, vec2 size, vec2 anchor, float spread, Placement start,
                 PanelSwap swap, vec3 tangent) {
    float transition = 0.0;
    int coordinate = min(source, LAST_TIER_POSITION);
    if (source != target) {
        // Match the settled pose at both ends of an ordinary tier move.
        // The transition itself still follows the instantaneous 3D velocity.
        const float blend_span = 0.15;
        if (source != OFFSCREEN && progress < blend_span) {
            transition = smoothstep(0.0, blend_span, progress);
        } else if (target != OFFSCREEN && progress > 1.0 - blend_span) {
            transition = 1.0 - smoothstep(1.0 - blend_span, 1.0, progress);
            coordinate = min(target, LAST_TIER_POSITION);
            start = tier_placement(coordinate, size, anchor, spread, swap);
        } else {
            return flight_orientation(tangent);
        }
    }

    // Both clocks advance together and wrap on exact sample boundaries.
    // Round the panel knot after removing the orbit fraction so a small
    // rounding difference between the clocks cannot shift it by one knot.
    float step = flock_motion.x / TURN_SAMPLE_SECONDS;
    float fraction = fract(step);
    float flight_time = floor(step) * TURN_SAMPLE_SECONDS;
    float panel_time = round(swap_time / TURN_SAMPLE_SECONDS - fraction) * TURN_SAMPLE_SECONDS;
    vec4 earlier = heading_at(coordinate, seed, speed_percent, depth_phase, size, anchor, spread,
                             start, swap, flight_time, panel_time);
    vec4 later = heading_at(coordinate, seed, speed_percent, depth_phase, size, anchor, spread,
                           start, swap, flight_time + TURN_SAMPLE_SECONDS,
                           panel_time + TURN_SAMPLE_SECONDS);
    // Sign-aligned quaternion interpolation takes at most half a turn in
    // half a second, including paths whose forward vector crosses vertical.
    vec4 settled = shortest_heading(earlier, later, fraction);
    if (transition > 0.0) {
        if (dot(tangent.xy, tangent.xy) < 1e-8) {
            tangent = vec3(0.0, 1.0, 0.0);
        }
        settled = shortest_heading(settled, flight_heading(tangent), transition);
    }
    return heading_pose(settled);
}

