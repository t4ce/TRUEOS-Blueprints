"""Extract the authored skirt heart with Blender's background Python runner."""

import argparse
from collections import Counter
import math
from pathlib import Path
from statistics import median
import struct
import sys

import bpy


OUTPUT = Path(__file__).resolve().parents[1] / "assets" / "skirt-heart-curve.bin"
RINGS = 72
SIDES = 3


def evaluated_loop(obj):
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = evaluated.to_mesh()
    try:
        return (
            [v.co.copy() for v in mesh.vertices],
            [tuple(edge.vertices) for edge in mesh.edges],
            [tuple(face.vertices) for face in mesh.polygons],
        )
    finally:
        evaluated.to_mesh_clear()


def wire_points(obj):
    # Use the original path through Geometry Nodes with the section removed.
    copy = obj.copy()
    bpy.context.scene.collection.objects.link(copy)
    group = obj.modifiers["Curve to Tube"].node_group.copy()
    try:
        copy.modifiers["Curve to Tube"].node_group = group
        curve_to_mesh = group.nodes["Curve to Mesh"]
        for link in list(curve_to_mesh.inputs["Profile Curve"].links):
            group.links.remove(link)
        output = next(node for node in group.nodes
                      if node.type == "GROUP_OUTPUT" and node.is_active_output)
        group.links.new(curve_to_mesh.outputs["Mesh"], output.inputs["Mesh"])
        bpy.context.view_layer.update()
        return evaluated_loop(copy)
    finally:
        bpy.data.objects.remove(copy, do_unlink=True)
        bpy.data.node_groups.remove(group)


def export(obj, output):
    if obj.type != "CURVE" or len(obj.data.splines) != 1 or not obj.data.splines[0].use_cyclic_u:
        raise ValueError("NurbsCircle must contain a single closed spline")
    tube, _, faces = evaluated_loop(obj)
    points, wire_edges, wire_faces = wire_points(obj)
    cycle = {frozenset((i, (i + 1) % RINGS)) for i in range(RINGS)}
    if (len(tube) != RINGS * SIDES or len(points) != RINGS or wire_faces
            or {frozenset(edge) for edge in wire_edges} != cycle
            or Counter(frozenset(i // SIDES for i in face) for face in faces)
            != Counter({edge: SIDES for edge in cycle})):
        raise ValueError("unexpected heart tube or wire topology")

    rings = list(zip(tube[::3], tube[1::3], tube[2::3], strict=True))
    if max(((a + b + c) / SIDES - p).length
           for (a, b, c), p in zip(rings, points, strict=True)) > 1e-6:
        raise ValueError("heart tube rings and authored curve samples disagree")

    def world(point):
        x, y, z = obj.matrix_world @ point
        return x, z, -y

    points = [world(p) for p in points]
    if any(a == b for a, b in zip(points, points[1:] + points[:1], strict=True)):
        raise ValueError("heart contour repeats a neighboring point")
    output.write_bytes(b"".join(struct.pack("<3f", *p) for p in points))

    widths = []
    for ring in rings:
        xy = [world(p)[:2] for p in ring]
        widths.append(max(math.dist(a, b) for a in xy for b in xy))
    print(f"{output}: {len(points)} points. Front section width median {1e3 * median(widths):.3f} mm")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
    export(bpy.data.objects["NurbsCircle"], args.output)


if __name__ == "__main__":
    main()
