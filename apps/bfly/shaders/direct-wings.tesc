#version 450 core

layout(vertices = 4) out;
layout(location = 0) flat in int panel_side[];
layout(location = 0) patch out int isoline_count;
layout(location = 1) patch out int straight_edge;
layout(location = 2) patch out vec3 clip_from_x;
layout(location = 3) patch out vec3 clip_from_y;
layout(location = 4) patch out vec3 clip_from_origin;
#ifdef BFLY_TWILIGHT_RIBBON
layout(location = 5) patch out vec3 clip_from_mid_x;
layout(location = 6) patch out vec3 clip_from_tail_x;
#endif

layout(std430, binding = 0) readonly buffer Camera {
    float records0[72];
    layout(offset = 288) vec4 framebuffer;
    layout(offset = 304) float records1[8];
    layout(offset = 336) vec4 flock_motion;
    layout(offset = 352) float swap_time;
#ifdef BFLY_MIXED_WINGS
    layout(offset = 356) float textured_trail;
#endif
#ifdef BFLY_TWILIGHT_RIBBON
    layout(offset = 364) float twilight_mode;
#endif
};

const int PANEL_COUNT = 8;
const int SPLIT_COUNT = PANEL_COUNT - 1;
const float SWAP_PERIOD = 30.0;
const float SWAP_DURATION = 4.0;
const float SWAP_CYCLE = 2.0 * float(SPLIT_COUNT) * SWAP_PERIOD;
const int TIER_FRACTIONS = 16;
const int LAST_TIER_POSITION = (PANEL_COUNT - 1) * TIER_FRACTIONS;
const int OFFSCREEN = 127;
const float FOCAL_LENGTH = 8000.0;
const float TAU = 6.2831853;
const float LN_2 = 0.6931472;
const vec3 SIDE_BOUNDS = vec3(-0.33208677881903803, 0.6622233225082153, 0.3311099279079387);
#ifdef BFLY_MIXED_WINGS
const vec3 SECOND_SIDE_BOUNDS = vec3(-0.3989315986135583, 0.6520235040581799, 0.2859345017392921);
#endif

struct PanelSwap {
    uint flipped;
    int depth;
    float blend;
    float blend_rate;
};

PanelSwap panel_swap() {
    float elapsed = mod(swap_time, SWAP_CYCLE) - 0.5 * SWAP_PERIOD;
    int event = max(0, int(floor(elapsed / SWAP_PERIOD)));
    int depth = event % SPLIT_COUNT;
    // Each pass flips all splits. The second pass restores the initial layout.
    uint previous_pass = event >= SPLIT_COUNT ? (1u << SPLIT_COUNT) - 1u : 0u;
    uint flipped = previous_pass ^ ((1u << depth) - 1u);
    float t = clamp((elapsed - float(event) * SWAP_PERIOD) / SWAP_DURATION, 0.0, 1.0);
    return PanelSwap(flipped, depth, t * t * (3.0 - 2.0 * t),
                     6.0 * t * (1.0 - t) / SWAP_DURATION);
}

vec4 split_panel(inout vec4 rest, out vec2 travel) {
    vec4 panel = rest;
    vec2 extent = rest.zw - rest.xy;
    if (extent.x >= extent.y) {
        travel = vec2(extent.x * 0.5, 0.0);
        panel.z = rest.x + travel.x;
        rest.x = panel.z;
    } else {
        travel = vec2(0.0, -extent.y * 0.5);
        panel.y = rest.w + travel.y;
        rest.w = panel.y;
    }
    return panel;
}

// xy is the singleton half's translation, zw its velocity. The other
// half takes their negatives, carrying each nested rectangle with it.
vec4 split_motion(PanelSwap swap, int depth, vec2 travel) {
    bool flipped = (swap.flipped & (1u << depth)) != 0u;
    float amount = flipped ? 1.0 : 0.0;
    float direction = flipped ? -1.0 : 1.0;
    float rate = 0.0;
    if (depth == swap.depth) {
        amount += direction * swap.blend;
        rate = direction * swap.blend_rate;
    }
    return vec4(travel * amount, travel * rate);
}

struct Panel {
    vec4 rect;
    vec4 motion;
};

void panel_pair(int index, vec2 size, PanelSwap swap, out Panel panel, out Panel next) {
    vec4 rest = vec4(0.0, 0.0, size);
    vec4 motion = vec4(0.0);
    vec2 travel;
    for (int split = 0; split < index; ++split) {
        split_panel(rest, travel);
        motion -= split_motion(swap, split, travel);
    }
    panel = Panel(rest, motion);
    if (index < SPLIT_COUNT) {
        panel.rect = split_panel(rest, travel);
        vec4 movement = split_motion(swap, index, travel);
        panel.motion += movement;
        motion -= movement;
    }
    next = Panel(rest, motion);
    if (index < SPLIT_COUNT - 1) {
        next.rect = split_panel(rest, travel);
        next.motion += split_motion(swap, index + 1, travel);
    }
}

vec2 flight_knot(int knot, int count, float angle_offset) {
    // Wrap the knot index before computing its angle so adjacent segments
    // share exactly the same endpoints and tangents across the lap seam.
    float angle = float((knot + count) % count) * (TAU / float(count)) + angle_offset;
    return vec2(cos(angle), 0.82 * sin(angle));
}

void flight_path(int seed, float speed_percent, float lap_seconds, float depth_phase,
                 out vec2 position, out vec2 tangent, out vec2 depth_drift) {
    int index = seed & 7;
    int count = 3 + index % 5;
    float offset = float(index) / float(PANEL_COUNT) + float(seed >> 3) * 0.618034;
    // An integer number of laps per hundred base laps preserves phase when
    // the flight clock wraps, even at each butterfly's own flight rate.
    float hundred_laps = 100.0 * lap_seconds;
    float cycle = mod(flock_motion.x, hundred_laps) / hundred_laps;
    float phase = fract(fract(cycle * speed_percent) + fract(offset));
    float progress = phase * float(count);
    int segment = int(progress);
    float t = fract(progress);
    float angle_offset = 0.6 * float(index);

    // Closed Catmull–Rom spline with a derivative in screen space.
    vec2 a = flight_knot(segment - 1, count, angle_offset);
    vec2 b = flight_knot(segment, count, angle_offset);
    vec2 c = flight_knot(segment + 1, count, angle_offset);
    vec2 d = flight_knot(segment + 2, count, angle_offset);
    vec2 start = 0.5 * (c - a);
    vec2 end = 0.5 * (d - b);
    vec2 span = c - b;
    vec2 quadratic = 3.0 * span - 2.0 * start - end;
    vec2 cubic = start + end - 2.0 * span;
    position = b + t * (start + t * (quadratic + t * cubic));
    tangent = (start + t * (2.0 * quadratic + 3.0 * t * cubic))
              * (float(count) * speed_percent / hundred_laps);

    // Drift through half a logarithmic stage in either direction. This
    // slower cycle also completes whole turns at the flight clock wrap.
    float drift_cycles = round(0.2 * speed_percent);
    float angle = TAU * fract(fract(cycle * drift_cycles) + depth_phase);
    depth_drift = vec2(0.5 * sin(angle), 0.5 * TAU * cos(angle) * drift_cycles / hundred_laps);
}

mat3 flight_orientation(vec3 tangent) {
    // An orbit and a tier transition can briefly oppose one another.
    if (dot(tangent.xy, tangent.xy) < 1e-8) {
        tangent = vec3(0.0, 1.0, 0.0);
    }
    vec3 forward = normalize(tangent);
    vec3 right = normalize(cross(forward, vec3(0.0, 0.0, 1.0)));
    return mat3(right, forward, cross(right, forward));
}

struct Placement {
    vec2 center;
    vec2 center_rate;
    float level;
    float screen_radius;
};

Placement tier_placement(int coordinate, vec2 size, vec2 anchor, float spread, PanelSwap swap) {
    int tier = coordinate / TIER_FRACTIONS;
    float fraction = float(coordinate % TIER_FRACTIONS) / float(TIER_FRACTIONS);
    Panel panel, next;
    panel_pair(tier, size, swap, panel, next);
    vec2 extent = panel.rect.zw - panel.rect.xy;
    vec2 next_extent = next.rect.zw - next.rect.xy;
    vec2 center = mix(mix(panel.rect.xy, panel.rect.zw, anchor) + panel.motion.xy,
                      mix(next.rect.xy, next.rect.zw, anchor) + next.motion.xy,
                      fraction);
    vec2 center_rate = mix(panel.motion.zw, next.motion.zw, fraction);
    float screen_radius = spread * mix(min(extent.x, extent.y),
                                       min(next_extent.x, next_extent.y), fraction);
    return Placement(center, center_rate, float(coordinate) / float(TIER_FRACTIONS), screen_radius);
}

void locate_butterfly(int source, int target, int seed, float progress, float speed_percent,
                      vec2 size, float base_depth,
                      out vec2 center, out float depth, out vec3 position, out vec3 tangent) {
    float speed = speed_percent * 0.01;
    uint variation = uint(seed) * 0x9e3779b9u;
    uint anchor_bits = variation + 0x9e3779b9u;
    vec2 anchor = vec2(float(anchor_bits & 65535u), float(anchor_bits >> 16)) / 65535.0;
    float spread = 0.13 + float(variation >> 24) * (0.06 / 255.0);
    float depth_phase = float((variation >> 16) & 255u) / 256.0;
    vec2 orbit, orbit_rate, depth_drift;
    flight_path(seed, speed_percent, max(framebuffer.w, 1.0), depth_phase, orbit, orbit_rate, depth_drift);

    int source_coordinate = min(source, LAST_TIER_POSITION);
    PanelSwap swap = panel_swap();
    Placement start = tier_placement(source_coordinate, size, anchor, spread, swap);
    center = start.center;
    vec2 center_rate = start.center_rate;
    float level = start.level;
    float level_rate = 0.0;
    float screen_radius = start.screen_radius;
    float screen_radius_rate = 0.0;
    if (target == OFFSCREEN) {
        // A seeded outward heading with zero extra speed at takeoff.
        float angle = TAU * float(variation & 65535u) / 65536.0;
        vec2 exit_plan = vec2(cos(angle), sin(angle)) * (3.0 * max(size.x, size.y));
        center += progress * progress * exit_plan;
        center_rate += exit_plan * (2.0 * progress * speed / max(flock_motion.y, 0.001));
    } else {
        int target_coordinate = min(target, LAST_TIER_POSITION);
        Placement finish = target_coordinate == source_coordinate
                         ? start : tier_placement(target_coordinate, size, anchor, spread, swap);
        if (source == OFFSCREEN) {
            start.level += log2(1.18);
        }
        float blend = progress * progress * (3.0 - 2.0 * progress);
        float blend_rate = 6.0 * progress * (1.0 - progress) * speed / max(flock_motion.z, 0.001);
        center = mix(start.center, finish.center, blend);
        center_rate = mix(start.center_rate, finish.center_rate, blend)
                      + (finish.center - start.center) * blend_rate;
        // Each tier doubles depth. Interpolate levels to preserve proportional growth.
        level = mix(start.level, finish.level, blend);
        level_rate = (finish.level - start.level) * blend_rate;
        screen_radius = mix(start.screen_radius, finish.screen_radius, blend);
        screen_radius_rate = (finish.screen_radius - start.screen_radius) * blend_rate;
    }

    depth = base_depth * exp2(level + depth_drift.x);
    float depth_rate = LN_2 * depth * (level_rate + depth_drift.y);
    // Convert the screen-space orbit to world units at the current depth
    // so approaching or receding does not move the projected hinge.
    float radius = screen_radius * depth / FOCAL_LENGTH;
    float radius_rate = (screen_radius_rate * depth + screen_radius * depth_rate) / FOCAL_LENGTH;
    position = vec3(orbit * radius, 0.0);
    // A fixed screen anchor follows its camera ray as depth changes.
    vec2 anchor_rate = (center_rate * depth + (center - 0.5 * size) * depth_rate) / FOCAL_LENGTH;
    tangent = vec3(anchor_rate + orbit_rate * radius + orbit * radius_rate, -depth_rate);
}

float fast_flap_weight(int source, int target, float progress, float speed_percent,
                       out float weight_rate) {
    weight_rate = 0.0;
    if (source == target || source == OFFSCREEN) {
        return 0.0;
    }
    bool departing = target == OFFSCREEN;
    float duration = departing ? flock_motion.y : flock_motion.z;
    // Express the 0.75-second blend in this butterfly's normalized move time.
    float ramp = 0.0075 * speed_percent / max(duration, 0.001);
    float entering = clamp(progress / ramp, 0.0, 1.0);
    float leaving = departing ? 1.0 : clamp((1.0 - progress) / ramp, 0.0, 1.0);
    float ramp_in = smoothstep(0.0, 1.0, entering);
    float ramp_out = smoothstep(0.0, 1.0, leaving);
    weight_rate = (6.0 / 0.75) *
        (entering * (1.0 - entering) * ramp_out - leaving * (1.0 - leaving) * ramp_in);
    return ramp_in * ramp_out;
}

vec2 flap_angles(int index, float time, float fast_weight, float weight_rate,
                 out bvec2 downstroke) {
    float offset = float(index) / float(PANEL_COUNT);
    float relaxed_phase = 0.25 * time + offset;
    vec2 relaxed_cycle = TAU * fract(vec2(relaxed_phase) + vec2(0.0, 0.135));
    vec2 relaxed = radians(32.0) * sin(relaxed_cycle);
    vec2 relaxed_rate = (radians(32.0) * TAU * 0.25) * cos(relaxed_cycle);
    if (fast_weight == 0.0) {
        downstroke = lessThan(relaxed_rate, vec2(0.0));
        return relaxed;
    }

    float beat = time / 1.25 + offset;
    // Five is coprime to sixteen, so each butterfly cycles through every sweep.
    int sweep_step = (int(beat) * 5 + index * 3) & 15;
    float sweep = 75.0 + 5.0 * float(sweep_step);
    // Pick the sweep at the common upper turnaround, where the velocity is zero.
    float beat_cycle = TAU * fract(beat);
    float sweep_progress = 0.5 - 0.5 * cos(beat_cycle);
    float synchronized = radians(87.75 - sweep * sweep_progress);
    float synchronized_rate = -radians(sweep) * (0.5 * TAU / 1.25) * sin(beat_cycle);
    vec2 rate = mix(relaxed_rate, vec2(synchronized_rate), fast_weight)
                + (vec2(synchronized) - relaxed) * weight_rate;
    // Both mirrored wings move downward as their local flap angle decreases.
    downstroke = lessThan(rate, vec2(0.0));

    return mix(relaxed, vec2(synchronized), fast_weight);
}

vec2 project(vec3 view, float depth) {
    return FOCAL_LENGTH * view.xy / (depth - view.z);
}

void projected_bounds(vec3 right, vec3 left, vec3 y_basis, vec3 position, float depth,
                      out vec2 lo, out vec2 hi
#ifdef BFLY_MIXED_WINGS
                      , vec3 side_bounds
#endif
) {
#ifndef BFLY_MIXED_WINGS
    const vec3 side_bounds = SIDE_BOUNDS;
#endif
    vec3 bottom = position + side_bounds.x * y_basis;
    vec3 top = position + side_bounds.z * y_basis;
    vec2 corners[6] = vec2[6](
        project(bottom, depth),
        project(top, depth),
        project(bottom + side_bounds.y * right, depth),
        project(top + side_bounds.y * right, depth),
        project(bottom + side_bounds.y * left, depth),
        project(top + side_bounds.y * left, depth)
    );
    lo = corners[0];
    hi = corners[0];
    for (int i = 1; i < 6; ++i) {
        lo = min(lo, corners[i]);
        hi = max(hi, corners[i]);
    }
}

vec3 clip_basis(vec3 axis, vec2 scale, vec2 center) {
    return vec3(axis.xy * scale - center * axis.z, -axis.z);
}

#ifdef BFLY_BREED_FILL
uint breed_fill_rgb(int seed, int artwork) {
    // The seed persists across growth, flock compaction, and both wing sides.
    uint shade = (uint(seed >> 1) * 37u + uint(artwork) * 17u + 13u) & 63u;
    float roll = (float(shade) + 0.5) / 64.0;
    float toward_red = roll * mix(0.45, 1.0, roll);
    vec3 hue;
    if (artwork == 0) {
        float angle = mix(58.0, 7.0, toward_red);
        hue = vec3(1.0, angle / 60.0, 0.0);
    } else {
        float angle = mix(288.0, 352.0, toward_red);
        hue = vec3(min(1.0, (angle - 240.0) / 60.0),
                   0.0, min(1.0, (360.0 - angle) / 60.0));
    }
    uvec3 rgb = uvec3(round(38.0 + 198.0 * hue));
    return rgb.r | (rgb.g << 8u) | (rgb.b << 16u);
}
#endif

void main() {
    gl_out[gl_InvocationID].gl_Position = gl_in[gl_InvocationID].gl_Position;
    if (gl_InvocationID != 0) {
        return;
    }

#ifdef BFLY_TIER_WIDTH
    // The draw's base instance selects the raster width. The low byte still
    // identifies the original butterfly and side (at most 160 instances).
    int width_pass = panel_side[0] >> 8;
    int index = (panel_side[0] & 255) >> 1;
#else
    int index = panel_side[0] >> 1;
#endif
    bool right_side = (panel_side[0] & 1) == 0;
#if defined(BFLY_FILL_WINGS)
    straight_edge = int(gl_in[0].gl_Position.z);
    int artwork = straight_edge >> 2;
#elif defined(BFLY_MIXED_WINGS)
    int patch_kind = int(gl_in[0].gl_Position.z);
#ifdef BFLY_INSET_OUTLINE
    int artwork = patch_kind >> 2;
#else
    int artwork = patch_kind >> 1;
#endif
    straight_edge = patch_kind & 1;
#else
    straight_edge = int(gl_in[0].gl_Position.z);
#endif
#ifdef BFLY_TWILIGHT_RIBBON
    if (index >= int(flock_motion.w) || index >= 80 || straight_edge != 0 || twilight_mode < 0.5) {
#elif defined(BFLY_FILL_WINGS)
    if (index >= int(flock_motion.w) || index >= 80) {
#else
    // Both mirrored sides meet at the same straight connector.
    if (index >= int(flock_motion.w) || index >= 80 || (straight_edge != 0 && !right_side)) {
#endif
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }
    // The top bits keep the carrier float finite. The remaining bits encode
    // the seed, fractional source/destination tiers, and transition progress.
    float record = index < 72 ? records0[index] : records1[index - 72];
    uint packed_record = floatBitsToUint(record);
    int seed = int((packed_record >> 22) & 127u);
#ifdef BFLY_MIXED_WINGS
    // Each seed keeps the same artwork while the flock moves and compacts.
    if (artwork != (seed & 1)) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }
#endif
    int source = int((packed_record >> 15) & 127u);
    int target = int((packed_record >> 8) & 127u);
    float progress = float(packed_record & 255u) / 255.0;
    // Match Flock's per-seed rate for flight, growth, and flap transitions.
    float speed_percent = float(85 + (seed * 17 + 13) % 31);
    vec4 frame = framebuffer;
    float time = mod(frame.z, 60.0);
    float fit = min(1.0, min(max(frame.x, frame.y) / 3072.0, min(frame.x, frame.y) / 1728.0));
    vec2 center_px;
    float depth;
    vec3 position, tangent;
    locate_butterfly(source, target, seed, progress, speed_percent, frame.xy, 6.0 / fit,
                     center_px, depth, position, tangent);
    mat3 pose = flight_orientation(tangent);
    float weight_rate;
    float fast_weight = fast_flap_weight(source, target, progress, speed_percent, weight_rate);
    bvec2 downstroke;
    vec2 angles = flap_angles(seed & 7, time, fast_weight, weight_rate, downstroke);
    vec2 across = cos(angles);
    vec2 depth_offset = sin(angles);
    vec3 right = pose * vec3(across.x, 0.0, depth_offset.x);
    vec3 left = pose * vec3(-across.y, 0.0, depth_offset.y);
    vec3 y_basis = pose[1];
    vec2 lo, hi;
    projected_bounds(right, left, y_basis, position, depth, lo, hi
#ifdef BFLY_MIXED_WINGS
                     , artwork == 0 ? SIDE_BOUNDS : SECOND_SIDE_BOUNDS
#endif
    );
    vec2 extent = hi - lo;
#if defined(BFLY_TWILIGHT_RIBBON)
    float span = max(extent.x, extent.y);
    if (span < 24.0) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

    bool long_sweep = twilight_mode >= 1.5;
    bool descending = right_side ? downstroke.x : downstroke.y;
    if (!long_sweep && !descending) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

    // Sweep the actual flap angle while holding the short-lived flight pose.
    // The middle sample bends the ribbon around the wing's hinge.
    float age = long_sweep ? 0.22 : 0.14;
    bvec2 unused_direction;
    vec2 mid_angles = flap_angles(seed & 7, mod(time - 0.5 * age, 60.0),
                                 fast_weight, weight_rate, unused_direction);
    vec2 tail_angles = flap_angles(seed & 7, mod(time - age, 60.0),
                                  fast_weight, weight_rate, unused_direction);
    float mid_angle = right_side ? mid_angles.x : mid_angles.y;
    float tail_angle = right_side ? tail_angles.x : tail_angles.y;
    float side = right_side ? 1.0 : -1.0;
    vec3 mid_x = pose * vec3(side * cos(mid_angle), 0.0, sin(mid_angle));
    vec3 tail_x = pose * vec3(side * cos(tail_angle), 0.0, sin(tail_angle));
    vec3 head_x = right_side ? right : left;
    float tip = (artwork == 0 ? SIDE_BOUNDS : SECOND_SIDE_BOUNDS).y;
    vec2 head_px = project(position + tip * head_x, depth);
    vec2 mid_px = project(position + tip * mid_x, depth);
    vec2 tail_px = project(position + tip * tail_x, depth);
    float sweep = length(head_px - mid_px) + length(mid_px - tail_px);
    float margin = 8.0 + 2.0 * sweep;
    if (sweep < 1.5 || any(lessThan(hi + center_px, vec2(-margin))) ||
        any(greaterThan(lo + center_px, frame.xy + margin))) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

    vec2 scale = (2.0 * FOCAL_LENGTH) / frame.xy;
    vec2 center = 2.0 * center_px / frame.xy - 1.0;
    clip_from_mid_x = clip_basis(mid_x, scale, center);
    clip_from_tail_x = clip_basis(tail_x, scale, center);
    // These two existing patch lanes carry appearance data for the ribbon.
    isoline_count = int(round(255.0 * smoothstep(1.5, 12.0, sweep))) | (int(long_sweep) << 8);
    straight_edge = seed;
    float segments = float(1 << findMSB(uint(clamp(span * 0.25, 8.0, 32.0))));
    gl_TessLevelOuter[0] = 2.0;
    gl_TessLevelOuter[1] = segments;
    gl_TessLevelOuter[2] = 2.0;
    gl_TessLevelOuter[3] = segments;
    gl_TessLevelInner[0] = segments;
    gl_TessLevelInner[1] = 2.0;
#elif defined(BFLY_FILL_WINGS)
    if (any(lessThan(hi + center_px, vec2(0.0))) ||
        any(greaterThan(lo + center_px, frame.xy))) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }
    // U follows the contour and V runs out from the interior of the wing.
    // Each straight closure contributes a single wedge. The quad domain
    // samples the cubics using the projected span, up to its 64-segment limit.
    uint span = uint(clamp(max(extent.x, extent.y), 4.0, 64.0));
    float segments = (straight_edge & 1) != 0 ? 1.0 : float(1 << findMSB(span));
    gl_TessLevelOuter[0] = 1.0;
    gl_TessLevelOuter[1] = segments;
    gl_TessLevelOuter[2] = 1.0;
    gl_TessLevelOuter[3] = segments;
    gl_TessLevelInner[0] = segments;
    gl_TessLevelInner[1] = 1.0;
#ifdef BFLY_BREED_FILL
    // The three low bits still identify the edge and its artwork.
    straight_edge |= int(breed_fill_rgb(seed, artwork) << 3u);
#endif
#else
    bool hinge_only = any(lessThan(extent, vec2(4.0)));
    float margin = hinge_only ? 2.0 : 1.0;
#ifdef BFLY_TIER_WIDTH
    uint span = uint(clamp(max(extent.x, extent.y), 4.0, 1024.0));
    int log_span = findMSB(span);
    int tier = hinge_only || span < 16u ? 7 : 10 - log_span;
    int width_class = max(0, 2 - tier);
    margin += 0.5 * float(width_class);
    bool wrong_width_pass = width_pass != width_class;
    if (wrong_width_pass || (hinge_only && straight_edge == 0) ||
#else
    if ((hinge_only && straight_edge == 0) ||
#endif
        any(lessThan(hi + center_px, vec2(-margin))) ||
        any(greaterThan(lo + center_px, frame.xy + margin))) {
        gl_TessLevelOuter[0] = 0.0;
        gl_TessLevelOuter[1] = 0.0;
        return;
    }

#ifndef BFLY_TIER_WIDTH
    uint span = uint(clamp(max(extent.x, extent.y), 4.0, 1024.0));
    int log_span = findMSB(span);
#endif
    int curve_segments = span < 16u ? 4 : 1 << log_span;
    int segments = (straight_edge != 0) ? 1 : curve_segments;
#ifndef BFLY_TIER_WIDTH
    int tier = hinge_only || span < 16u ? 7 : 10 - log_span;
#endif
    bool deposit = straight_edge == 0 && (right_side ? downstroke.x : downstroke.y);
    // Keep the connector kind in the low two bits, and carry the texture
    // strip and downstroke bit alongside it to the fragment stage.
    int edge_kind = hinge_only ? 2 : straight_edge;
#ifdef BFLY_MIXED_WINGS
    straight_edge = textured_trail > 0.0 ? edge_kind | (tier << 2) | (deposit ? 32 : 0) : edge_kind;
#else
    straight_edge = edge_kind | (tier << 2) | (deposit ? 32 : 0);
#endif
    // Every isoline covers one interval of the same cubic.
    isoline_count = max(1, segments / 64);
    gl_TessLevelOuter[0] = float(isoline_count);
    gl_TessLevelOuter[1] = float(segments / isoline_count);
#endif

    vec2 scale = (2.0 * FOCAL_LENGTH) / frame.xy;
    vec2 center = 2.0 * center_px / frame.xy - 1.0;
    vec3 x_basis = right_side ? right : left;
#if !defined(BFLY_FILL_WINGS) && !defined(BFLY_TWILIGHT_RIBBON)
    if (hinge_only) {
        // Keep a two-pixel centerline at the projected hinge midpoint even
        // when the authored connector is too short to cover a sample.
        vec2 hinge = 0.5 * (gl_in[0].gl_Position.xy + gl_in[3].gl_Position.xy);
        vec3 root = position + x_basis * hinge.x + y_basis * hinge.y;
        vec2 midpoint = center + 2.0 * project(root, depth) / frame.xy;
        vec2 direction = y_basis.xy;
        if (dot(direction, direction) < 1e-8) {
            direction = vec2(0.0, 1.0);
        }
        clip_from_x = vec3(0.0);
        clip_from_y = vec3(4.0 * normalize(direction) / frame.xy, 0.0);
        clip_from_origin = vec3(midpoint, 1.0) - 0.5 * clip_from_y;
        return;
    }
#endif
    clip_from_x = clip_basis(x_basis, scale, center);
    clip_from_y = clip_basis(y_basis, scale, center);
    clip_from_origin = clip_basis(position, scale, center) + vec3(center, 1.0) * depth;
}
