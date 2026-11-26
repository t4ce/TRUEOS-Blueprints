#!/usr/bin/env python3
"""Assemble the live flight-heading variant without changing archived GLSL."""

from pathlib import Path
import sys


SHADERS = Path(__file__).resolve().parents[1] / "shaders"


def flight_turn_source(root=SHADERS):
    source = (root / "direct-wings.tesc").read_text()
    heading = (root / "direct-flight-turn.glsl").read_text()
    hooks = (
        ("PanelSwap panel_swap() {", "PanelSwap panel_swap(float panel_time) {"),
        ("mod(swap_time, SWAP_CYCLE)", "mod(panel_time, SWAP_CYCLE)"),
        ("PanelSwap swap = panel_swap();", "PanelSwap swap = panel_swap(swap_time);"),
        ("float lap_seconds, float depth_phase,\n",
         "float lap_seconds, float depth_phase, float flight_time,\n"),
        ("mod(flock_motion.x, hundred_laps)", "mod(flight_time, hundred_laps)"),
        ("depth_phase, orbit, orbit_rate, depth_drift);",
         "depth_phase, flock_motion.x, orbit, orbit_rate, depth_drift);"),
        ("void locate_butterfly(", f"{heading.rstrip()}\n\nvoid locate_butterfly("),
        ("out float depth, out vec3 position, out vec3 tangent)",
         "out float depth, out vec3 position, out mat3 pose)"),
        ("    tangent = vec3(anchor_rate + orbit_rate * radius + orbit * radius_rate, -depth_rate);",
         "    vec3 tangent = vec3(anchor_rate + orbit_rate * radius + orbit * radius_rate, -depth_rate);\n"
         "    pose = flight_pose(source, target, seed, progress, speed_percent, depth_phase, size,\n"
         "                       anchor, spread, start, swap, tangent);"),
        ("    vec3 position, tangent;", "    vec3 position;\n    mat3 pose;"),
        ("                     center_px, depth, position, tangent);\n"
         "    mat3 pose = flight_orientation(tangent);",
         "                     center_px, depth, position, pose);"),
    )
    for original, updated in hooks:
        if source.count(original) != 1:
            raise ValueError(f"Expected one flight shader hook for {original!r}")
        source = source.replace(original, updated, 1)
    return source


if __name__ == "__main__":
    sys.stdout.write(flight_turn_source())
