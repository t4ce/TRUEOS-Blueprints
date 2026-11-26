#!/usr/bin/env python3
"""Compose the stage wake hull shader from the active flight shader."""

import sys


def stage_wake_source(flight_source):
    replacements = (
        ("layout(location = 6) patch out vec3 clip_from_tail_x;",
         "layout(location = 6) patch out vec3 clip_from_tail_x;\n"
         "#ifdef BFLY_SUNRISE\n"
         "layout(location = 7) patch out vec3 flight_wake;\n"
         "#endif"),
        ("#ifdef BFLY_MIXED_WINGS\n    layout(offset = 356)",
         "#if defined(BFLY_MIXED_WINGS) && !defined(BFLY_SUNRISE)\n    layout(offset = 356)"),
        ("#ifdef BFLY_TWILIGHT_RIBBON\n    layout(offset = 364) float twilight_mode;\n#endif",
         "#ifdef BFLY_SUNRISE\n"
         "    layout(offset = 356) float sunrise_weight;\n"
         "    layout(offset = 360) float twilight_weight;\n"
         "    layout(offset = 364) float universe_weight;\n"
         "#elif defined(BFLY_TWILIGHT_RIBBON)\n"
         "    layout(offset = 364) float twilight_mode;\n"
         "#endif"),
        ("out float depth, out vec3 position, out mat3 pose)",
         "out float depth, out vec3 position, out mat3 pose, out vec2 hinge_rate)"),
        ("    position = vec3(orbit * radius, 0.0);",
         "    position = vec3(orbit * radius, 0.0);\n"
         "    hinge_rate = center_rate + orbit_rate * screen_radius + orbit * screen_radius_rate;"),
        ("    mat3 pose;\n    locate_butterfly(",
         "    mat3 pose;\n    vec2 hinge_rate;\n    locate_butterfly("),
        ("                     center_px, depth, position, pose);",
         "                     center_px, depth, position, pose, hinge_rate);"),
        ("    const float age = 0.590;\n"
         "    bool twilight_sky = twilight_mode >= 1.5;\n"
         "    float stretch = twilight_sky ? 0.75 : 0.8;",
         "    float age = mix(0.590, 0.600, universe_weight);\n"
         "    float stretch = dot(vec3(0.8, 0.75, 0.65),\n"
         "                        vec3(sunrise_weight, twilight_weight, universe_weight));"),
        ("    float sweep = length(head_px - mid_px) + length(mid_px - tail_px);",
         "    float sweep = length(head_px - mid_px) + length(mid_px - tail_px);\n"
         "#ifdef BFLY_SUNRISE\n"
         "    // Flight takes a share of the sweep instead of adding to its peak.\n"
         "    // Permit a short wake at zero flap speed, using the existing 12 px full-gain reference.\n"
         "    const float full_sweep_px = 12.0;\n"
         "    const float max_flight_share = 0.35;\n"
         "    vec2 travel = -hinge_rate * age;\n"
         "    float flight_px = length(travel);\n"
         "    float share = max_flight_share * smoothstep(0.0, full_sweep_px, flight_px);\n"
         "    float budget = max(sweep, full_sweep_px);\n"
         "    float bounded_px = min(flight_px, budget);\n"
         "    vec2 tail_offset = travel * (share * bounded_px / max(flight_px, 0.0001));\n"
         "    flight_wake = vec3(2.0 * tail_offset / frame.xy, share);\n"
         "    sweep = mix(sweep, bounded_px, share);\n"
         "#endif"),
        ("    tag |= (uint(twilight_sky) << 5u) | (uint(artwork) << 1u) | uint(!right_side);",
         "    tag |= (uint(artwork) << 1u) | uint(!right_side);\n"
         "    uint sunrise = uint(round(255.0 * sunrise_weight));\n"
         "    uint twilight = min(255u - sunrise, uint(round(255.0 * twilight_weight)));\n"
         "    uint phase_bits = uint(round(2047.0 * wing_phase / TAU));"),
        ("    straight_edge = int((tag << 16u) | packHalf2x16(vec2(wing_phase, 0.0)));",
         "    // Eleven phase bits, five contour bits, and two eight-bit weights.\n"
         "    straight_edge = int((twilight << 24u) | (sunrise << 16u) | (tag << 11u) | phase_bits);"),
    )
    for original, replacement in replacements:
        if flight_source.count(original) != 1:
            raise ValueError(f"Expected one stage wake hull shader hook for {original!r}")
        flight_source = flight_source.replace(original, replacement, 1)
    return flight_source


if __name__ == "__main__":
    from bake_flight_turn import flight_turn_source

    sys.stdout.write(stage_wake_source(flight_turn_source()))
