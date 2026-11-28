"""Regenerate the native accessory cutout from the embedded avatar and packed mask."""

import argparse
import json
import math
from pathlib import Path
import struct

from PIL import Image

from build_eye_cutout import ALPHA_CUTOFF, QUANTIZATION, area_squared, clip_rectangle, cross


ASSETS = Path(__file__).resolve().parents[1] / "assets"
HAIR_MATERIAL = "N00_000_Hair_00_HAIR_01 (Instance)"
BAND_SIZE = (215, 268)
CLIP_CROP = (0, 1632, 312, 1940)
CLIP_ORIGIN = (BAND_SIZE[0], 0)
ATLAS_SIZE = (BAND_SIZE[0] + (CLIP_CROP[2] - CLIP_CROP[0]) // 2, BAND_SIZE[1])
# The band and the 47 faces of the Blender accessory split share the Hair image.
# Each half-resolution patch records its source crop and packed atlas origin.
PATCHES = (
    (range(3259, 3427), (754, 1514), (0, 0)),
    (range(4162, 4170), (0, 1672), (158, 0)),
    (range(526, 559), CLIP_CROP[:2], CLIP_ORIGIN),
    (range(3235, 3259), CLIP_CROP[:2], CLIP_ORIGIN),
)


def read_hair(path):
    glb = path.read_bytes()
    if struct.unpack_from("<4sII", glb) != (b"glTF", 2, len(glb)):
        raise ValueError("invalid avatar GLB header")
    length, kind = struct.unpack_from("<I4s", glb, 12)
    end = 20 + length
    if kind != b"JSON" or glb[end + 4:end + 8] != b"BIN\0":
        raise ValueError("missing JSON or BIN chunk")
    document = json.loads(glb[20:end])
    binary = memoryview(glb)[end + 8:]
    (hair,) = (
        part
        for mesh in document["meshes"]
        for part in mesh["primitives"]
        if document["materials"][part["material"]]["name"] == HAIR_MATERIAL
    )

    def values(index):
        accessor = document["accessors"][index]
        view = document["bufferViews"][accessor["bufferView"]]
        scalar = {5123: "H", 5125: "I", 5126: "f"}[accessor["componentType"]]
        components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3}[accessor["type"]]
        fmt = struct.Struct(f"<{components}{scalar}")
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", fmt.size)
        return [fmt.unpack_from(binary, offset + stride * i) for i in range(accessor["count"])]

    attrs = hair["attributes"]
    indices = [index for (index,) in values(hair["indices"])]
    triangles = list(zip(indices[::3], indices[1::3], indices[2::3], strict=True))
    return values(attrs["POSITION"]), values(attrs["TEXCOORD_0"]), triangles


def packed_uv(u, v, crop, origin):
    # Fit from embedded GLB UVs to pixels in the Blender Hair image. The
    # maximum error across the selected vertices is below 0.0001 source pixel.
    x = 2423.343360565823 * u - 1207.5302461756564
    y = 2422.3530352237667 * v - 303.38564347216993
    return ((x - crop[0]) / 2 + origin[0], (y - crop[1]) / 2 + origin[1])


def pack_clips(atlas, source):
    if CLIP_CROP[2] > source.width or CLIP_CROP[3] > source.height:
        raise ValueError("Hair image does not contain the accessory crop")
    crop = source.convert("RGBA").crop(CLIP_CROP)
    width, height = crop.width // 2, crop.height // 2
    reduced = Image.new("RGBA", (width, height))
    for y in range(height):
        for x in range(width):
            samples = [crop.getpixel((2 * x + dx, 2 * y + dy)) for dy in range(2) for dx in range(2)]
            weight = sum(pixel[3] for pixel in samples)
            if weight:
                rgb = tuple(
                    (sum(pixel[c] * pixel[3] for pixel in samples) + weight // 2) // weight
                    for c in range(3)
                )
                reduced.putpixel((x, y), (*rgb, (weight + 2) // 4))

    # Pad the cutout edges by one texel for linear filtering.
    padded = reduced.copy()
    for y in range(height):
        for x in range(width):
            if reduced.getpixel((x, y))[3]:
                continue
            neighbors = (
                reduced.getpixel((nx, ny))
                for ny in range(max(0, y - 1), min(height, y + 2))
                for nx in range(max(0, x - 1), min(width, x + 2))
            )
            rgb = max(neighbors, key=lambda pixel: pixel[3])[:3]
            padded.putpixel((x, y), (*rgb, 0))

    packed = Image.new("RGBA", ATLAS_SIZE)
    packed.paste(atlas.crop((0, 0, *BAND_SIZE)), (0, 0))
    packed.paste(padded, CLIP_ORIGIN)
    return packed


def opaque_rectangles(mask):
    """Join matching opaque runs in consecutive rows of the packed mask."""
    width, height = mask.size
    pixels = mask.tobytes()
    active = {}
    for y in range(height + 1):
        runs = set()
        start = None
        for x, alpha in enumerate(pixels[y * width:(y + 1) * width] + b"\0"):
            if alpha >= ALPHA_CUTOFF and start is None:
                start = x
            elif alpha < ALPHA_CUTOFF and start is not None:
                runs.add((start, x))
                start = None
        for left, right in sorted(active.keys() - runs):
            yield (left, active.pop((left, right)), right, y)
        for run in sorted(runs - active.keys()):
            active[run] = y


def build(positions, source_uv, triangles, mask):
    packed = {
        i: packed_uv(*source_uv[i], crop, origin)
        for vertices, crop, origin in PATCHES
        for i in vertices
    }
    selected = [triangle for triangle in triangles if all(i in packed for i in triangle)]
    if len(selected) != 260 or set(packed) != {i for triangle in selected for i in triangle}:
        raise ValueError("unexpected hair accessory topology")
    if any(
        any(i in packed for i in triangle) and not all(i in packed for i in triangle)
        for triangle in triangles
    ):
        raise ValueError("accessories and hair share a render vertex")

    rectangles = list(opaque_rectangles(mask))
    vertices = []
    keys = {}
    output_uv = []
    output_positions = []
    indices = []
    emitted = set()

    def vertex(point, source, a, b, c, area):
        weights = [cross(point, b, c) / area, cross(a, point, c) / area, cross(a, b, point) / area]
        weights = [round(max(0, min(w, 1)) * QUANTIZATION) for w in weights]
        largest = max(range(3), key=weights.__getitem__)
        weights[largest] += QUANTIZATION - sum(weights)
        influences = sorted(zip(source, weights), key=lambda pair: (pair[1] != 0, pair[0]))
        uv = tuple(round(p / size * QUANTIZATION) for p, size in zip(point, ATLAS_SIZE))
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
                sum(positions[i][axis] * weight for i, weight in influences) / QUANTIZATION
                for axis in range(3)
            ))
        return index

    for source in selected:
        a, b, c = triangle = tuple(packed[i] for i in source)
        area = cross(a, b, c)
        if not math.isfinite(area) or abs(area) < 1e-7:
            raise ValueError("degenerate hair accessory UV triangle")
        winding = math.copysign(1, area)
        low_x, low_y = map(min, zip(*triangle))
        high_x, high_y = map(max, zip(*triangle))
        for rect in rectangles:
            left, top, right, bottom = rect
            if left >= high_x or right <= low_x or top >= high_y or bottom <= low_y:
                continue
            polygon = clip_rectangle(triangle, rect)
            for i in range(1, len(polygon) - 1):
                clipped = (polygon[0], polygon[i], polygon[i + 1])
                if cross(*clipped) * winding < 1e-7:
                    continue
                ids = [vertex(p, source, a, b, c, area) for p in clipped]
                if len(set(ids)) != 3 or cross(*(output_uv[j] for j in ids)) * winding < 1e-7:
                    continue
                if area_squared(*(output_positions[j] for j in ids)) < 1e-28:
                    continue
                emitted.add(source)
                indices.extend(ids)

    if len(emitted) != len(selected):
        raise ValueError("accessory mask leaves a source triangle unrepresented")

    used = sorted(set(indices))
    remap = {index: dense for dense, index in enumerate(used)}
    return [vertices[i] for i in used], [remap[i] for i in indices]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ASSETS / "AvatarSample_B-hairband-cutout.bin")
    parser.add_argument("--source-hair", type=Path, help="original packed Blender Hair PNG for the clip crop")
    args = parser.parse_args()
    positions, uv, triangles = read_hair(ASSETS / "AvatarSample_B.glb")
    atlas_path = ASSETS / "AvatarSample_B-hairband.png"
    with Image.open(atlas_path) as image:
        atlas = image.copy()
    if atlas.mode != "RGBA" or atlas.size not in (BAND_SIZE, ATLAS_SIZE):
        raise ValueError("unexpected packed accessory atlas format")
    if args.source_hair:
        with Image.open(args.source_hair) as source:
            atlas = pack_clips(atlas, source)
    if atlas.size != ATLAS_SIZE:
        raise ValueError("accessory atlas is missing the clip crop")
    vertices, indices = build(positions, uv, triangles, atlas.getchannel("A"))
    if args.source_hair:
        atlas.save(atlas_path, optimize=True)
    payload = bytearray(struct.pack("<II", len(vertices), len(indices)))
    for vertex in vertices:
        payload.extend(struct.pack("<IIIffff", *vertex))
    for index in indices:
        payload.extend(struct.pack("<I", index))
    args.output.write_bytes(payload)
    print(f"{args.output.name}: {len(vertices)} vertices, {len(indices) // 3} triangles, {len(payload)} bytes")


if __name__ == "__main__":
    main()
