"""Regenerate the native eyebrow/lash mesh from the authored UVs and packed mask."""

import json
import math
import struct
from pathlib import Path

from PIL import Image


ASSETS = Path(__file__).resolve().parents[1] / "assets"
FACE_SKIN = "N00_000_00_Face_00_SKIN (Instance)"
EYE_HIGHLIGHT = "N00_000_00_EyeHighlight_00_EYE (Instance)"
SOURCE_SIZE = 4096
ATLAS_SIZE = (1738, 309)
MASK_SIZE = (435, 78)
ALPHA_CUTOFF = 128
# Source atlas [left, top, right, bottom] and packed atlas origin [x, y].
REGIONS = [((2204, 770, 3942, 856), (0, 0)), ((2204, 2652, 3942, 2871), (0, 90))]
QUANTIZATION = 1 << 22


def read_avatar(path):
    glb = path.read_bytes()
    magic, version, size = struct.unpack_from("<4sII", glb)
    if (magic, version, size) != (b"glTF", 2, len(glb)):
        raise ValueError("invalid avatar GLB header")
    length, kind = struct.unpack_from("<I4s", glb, 12)
    end = 20 + length
    if kind != b"JSON" or glb[end + 4:end + 8] != b"BIN\0":
        raise ValueError("missing JSON or BIN chunk")
    document = json.loads(glb[20:end])
    binary = memoryview(glb)[end + 8:]

    def primitive(name):
        return next(
            section
            for mesh in document["meshes"]
            for section in mesh["primitives"]
            if document["materials"][section["material"]]["name"] == name
        )

    def values(index):
        accessor = document["accessors"][index]
        view = document["bufferViews"][accessor["bufferView"]]
        scalar = {5123: "H", 5125: "I", 5126: "f"}[accessor["componentType"]]
        count = {"SCALAR": 1, "VEC2": 2, "VEC3": 3}[accessor["type"]]
        fmt = struct.Struct(f"<{count}{scalar}")
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", fmt.size)
        return [fmt.unpack_from(binary, offset + stride * i) for i in range(accessor["count"])]

    eyes = primitive(EYE_HIGHLIGHT)
    position = eyes["attributes"]["POSITION"]
    if primitive(FACE_SKIN)["attributes"]["POSITION"] != position:
        raise ValueError("eye and skin primitives do not share face positions")
    indices = [index for (index,) in values(eyes["indices"])]
    triangles = list(zip(indices[::3], indices[1::3], indices[2::3], strict=True))
    return values(position), values(eyes["attributes"]["TEXCOORD_0"]), triangles


def mask_rectangles(mask):
    """Merge runs in successive rows. Rectangles are in full-size atlas pixels."""
    width, height = mask.size
    pixels = mask.tobytes()
    active = {}
    for y in range(height + 1):
        row = pixels[y * width:(y + 1) * width]
        row_runs = set()
        start = None
        for x, alpha in enumerate(row + b"\0"):
            if alpha >= ALPHA_CUTOFF and start is None:
                start = x
            elif alpha < ALPHA_CUTOFF and start is not None:
                row_runs.add((start, x))
                start = None
        for left, right in sorted(active.keys() - row_runs):
            top = active.pop((left, right))
            yield (left * ATLAS_SIZE[0] / width, top * ATLAS_SIZE[1] / height,
                   right * ATLAS_SIZE[0] / width, y * ATLAS_SIZE[1] / height)
        for run in sorted(row_runs - active.keys()):
            active[run] = y


def cross(a, b, c):
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def area_squared(a, b, c):
    u = [b[i] - a[i] for i in range(3)]
    v = [c[i] - a[i] for i in range(3)]
    return sum((u[i] * v[j] - u[j] * v[i]) ** 2 for i, j in ((0, 1), (0, 2), (1, 2)))


def clip(polygon, axis, plane, side):
    output = []
    for a, b in zip(polygon, polygon[1:] + polygon[:1]):
        x, y = (a[axis] - plane) * side, (b[axis] - plane) * side
        if x >= 0:
            output.append(a)
        if (x >= 0) != (y >= 0):
            t = x / (x - y)
            output.append((a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])))
    return output


def clip_rectangle(triangle, rect):
    left, top, right, bottom = rect
    result = list(triangle)
    for axis, plane, side in ((0, left, 1), (0, right, -1), (1, top, 1), (1, bottom, -1)):
        result = clip(result, axis, plane, side)
    return result


def lower_dash_centers(mask):
    """Pick a covered pixel center from each separate lower lash mark."""
    width, height = mask.size
    remaining = {
        (x, y)
        for y in range(62, height)
        for x in range(width)
        if mask.getpixel((x, y)) >= ALPHA_CUTOFF
    }
    centers = []
    while remaining:
        points = [remaining.pop()]
        for x, y in points:
            for point in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                if point in remaining:
                    remaining.remove(point)
                    points.append(point)
        center = tuple(sum(point[i] for point in points) / len(points) for i in range(2))
        x, y = min(points, key=lambda point: math.dist(point, center))
        centers.append(((x + 0.5) * ATLAS_SIZE[0] / width, (y + 0.5) * ATLAS_SIZE[1] / height))
    return centers


def inside(point, triangle):
    a, b, c = triangle
    sign = math.copysign(1, cross(a, b, c))
    return min(sign * cross(a, b, point), sign * cross(b, c, point), sign * cross(c, a, point)) >= -1e-8


def build(positions, source_uv, triangles, mask):
    rectangles = list(mask_rectangles(mask))
    vertices = []
    keys = {}
    output_uv = []
    output_positions = []
    indices = []
    selected = [0, 0]

    def vertex(point, source, a, b, c, area):
        weights = [cross(point, b, c) / area, cross(a, point, c) / area, cross(a, b, point) / area]
        weights = [round(max(0, min(w, 1)) * QUANTIZATION) for w in weights]
        largest = max(range(3), key=weights.__getitem__)
        weights[largest] += QUANTIZATION - sum(weights)
        influences = sorted(zip(source, weights), key=lambda pair: (pair[1] != 0, pair[0]))
        uv = tuple(round(p / size * QUANTIZATION) for p, size in zip(point, ATLAS_SIZE))
        # Zero weights let the same original vertex/edge share an output vertex
        # even when it comes from different adjacent source triangles.
        key = (tuple(pair for pair in influences if pair[1]), uv)
        index = keys.get(key)
        if index is None:
            index = len(vertices)
            keys[key] = index
            corners, weights = zip(*influences)
            target_uv = tuple(coord / QUANTIZATION for coord in uv)
            vertices.append((*corners, weights[0] / QUANTIZATION, weights[1] / QUANTIZATION, *target_uv))
            output_uv.append(tuple(coord * size for coord, size in zip(target_uv, ATLAS_SIZE)))
            output_positions.append(tuple(
                sum(positions[corner][axis] * weight for corner, weight in influences) / QUANTIZATION
                for axis in range(3)
            ))
        return index

    for source in triangles:
        authored_uv = [source_uv[i] for i in source]
        if not all(u > 0.5 for u, _ in authored_uv):
            continue
        region = int(all(v > 0.5 for _, v in authored_uv))
        selected[region] += 1
        (left, top, right, bottom), (x, y) = REGIONS[region]
        limit_x = x + right - left
        limit_y = y + bottom - top
        a, b, c = packed = tuple((u * SOURCE_SIZE - left + x, v * SOURCE_SIZE - top + y) for u, v in authored_uv)
        area = cross(a, b, c)
        world_area = area_squared(*(positions[i] for i in source))
        if not math.isfinite(area) or abs(area) < 1e-7 or not math.isfinite(world_area) or world_area < 1e-22:
            continue
        px, py = zip(*packed)
        low_x, low_y, high_x, high_y = min(px), min(py), max(px), max(py)
        for rx0, ry0, rx1, ry1 in rectangles:
            rx0, ry0, rx1, ry1 = max(rx0, x), max(ry0, y), min(rx1, limit_x), min(ry1, limit_y)
            if rx0 >= min(rx1, high_x) or rx1 <= low_x or ry0 >= min(ry1, high_y) or ry1 <= low_y:
                continue
            polygon = clip_rectangle(packed, (rx0, ry0, rx1, ry1))
            for i in range(1, len(polygon) - 1):
                subtriangle = (polygon[0], polygon[i], polygon[i + 1])
                if cross(*subtriangle) * math.copysign(1, area) < 1e-7:
                    continue
                ids = [vertex(p, source, a, b, c, area) for p in subtriangle]
                if len(set(ids)) != 3 or cross(*(output_uv[j] for j in ids)) * math.copysign(1, area) < 1e-7:
                    continue
                if area_squared(*(output_positions[j] for j in ids)) < 1e-28:
                    continue
                indices.extend(ids)

    if selected != [34, 233] or not indices:
        raise ValueError(f"unexpected eyebrow or lash topology: {selected}")
    centers = lower_dash_centers(mask)
    posed_uv = [tuple(output_uv[i] for i in ids) for ids in zip(indices[::3], indices[1::3], indices[2::3])]
    if len(centers) != 10 or not all(any(inside(center, triangle) for triangle in posed_uv) for center in centers):
        raise ValueError("cutout must cover all ten lower lash marks")

    # Drop clipped corner vertices unused after quantization.
    used = sorted(set(indices))
    remap = {index: dense for dense, index in enumerate(used)}
    return [vertices[i] for i in used], [remap[i] for i in indices]


def main():
    positions, uv, triangles = read_avatar(ASSETS / "AvatarSample_B.glb")
    with Image.open(ASSETS / "AvatarSample_B-eyebrows-and-lashes.png") as atlas:
        if atlas.size != ATLAS_SIZE or atlas.mode != "RGBA":
            raise ValueError("unexpected packed eye atlas format")
        mask = atlas.getchannel("A").resize(MASK_SIZE, Image.Resampling.BOX)
    vertices, indices = build(positions, uv, triangles, mask)
    output = ASSETS / "AvatarSample_B-eye-cutout.bin"
    packed = bytearray(struct.pack("<II", len(vertices), len(indices)))
    for vertex in vertices:
        packed.extend(struct.pack("<IIIffff", *vertex))
    for index in indices:
        packed.extend(struct.pack("<I", index))
    output.write_bytes(packed)
    print(f"{output.name}: {len(vertices)} vertices, {len(indices) // 3} triangles, {len(packed)} bytes")


if __name__ == "__main__":
    main()
