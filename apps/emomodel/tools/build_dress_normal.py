"""Restore the skirt's original UVs and derive fabric relief from AvatarSample_B.vrm."""

import argparse
from collections import deque
import io
import json
import math
from pathlib import Path
import struct

from PIL import Image, ImageFilter


ASSETS = Path(__file__).resolve().parents[1] / "assets"
MATERIAL = "N00_001_03_Bottoms_01_CLOTH (Instance)"
POSITION_TOLERANCE = 2e-7
UV_TOLERANCE = 2e-5
SIZE = 128
SLOPE_STRENGTH = 1.25


def read_glb(path):
    raw = path.read_bytes()
    if struct.unpack_from("<4sII", raw) != (b"glTF", 2, len(raw)):
        raise ValueError(f"invalid GLB header in {path}")
    size, kind = struct.unpack_from("<I4s", raw, 12)
    start = 20 + size
    if kind != b"JSON" or raw[start + 4:start + 8] != b"BIN\0":
        raise ValueError(f"missing JSON or BIN chunk in {path}")
    return json.loads(raw[20:start]), raw[start + 8:]


def accessor(document, binary, index):
    entry = document["accessors"][index]
    view = document["bufferViews"][entry["bufferView"]]
    components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3}[entry["type"]]
    scalar = {5123: "H", 5125: "I", 5126: "f"}[entry["componentType"]]
    fmt = struct.Struct(f"<{components}{scalar}")
    start = view.get("byteOffset", 0) + entry.get("byteOffset", 0)
    stride = view.get("byteStride", fmt.size)
    return [fmt.unpack_from(binary, start + stride * index) for index in range(entry["count"])]


def skirt_part(document):
    (primitive,) = (
        part for mesh in document["meshes"] for part in mesh["primitives"]
        if document["materials"][part["material"]]["name"] == MATERIAL
    )
    return primitive


def triangles(document, binary, primitive):
    indices = [index for (index,) in accessor(document, binary, primitive["indices"])]
    return list(zip(indices[::3], indices[1::3], indices[2::3], strict=True))


def difference(a, b):
    return tuple(left - right for left, right in zip(a, b, strict=True))


def dot(a, b):
    return sum(left * right for left, right in zip(a, b, strict=True))


def source_face(points, texcoords):
    a, b, c = points
    ab, ac = difference(b, a), difference(c, a)
    aa, bb, ab_ac = dot(ab, ab), dot(ac, ac), dot(ab, ac)
    det = aa * bb - ab_ac * ab_ac
    if det <= 0.0:
        raise ValueError("degenerate source garment triangle")
    bounds = tuple((min(axis), max(axis)) for axis in zip(*points))
    return a, ab, ac, aa, bb, ab_ac, det, bounds, texcoords


def face_uv(point, face):
    a, ab, ac, aa, bb, ab_ac, det, bounds, texcoords = face
    if any(not low - POSITION_TOLERANCE <= value <= high + POSITION_TOLERANCE
           for value, (low, high) in zip(point, bounds, strict=True)):
        return None
    target = difference(point, a)
    ab_p, ac_p = dot(ab, target), dot(ac, target)
    b = (bb * ab_p - ab_ac * ac_p) / det
    c = (aa * ac_p - ab_ac * ab_p) / det
    weights = 1.0 - b - c, b, c
    if min(weights) < -UV_TOLERANCE:
        return None
    if math.dist(target, tuple(b * x + c * y for x, y in zip(ab, ac))) > POSITION_TOLERANCE:
        return None
    return tuple(dot(weights, axis) for axis in zip(*texcoords))


def restore_uv(source, source_bin, dress, dress_bin):
    authored, clipped = skirt_part(source), skirt_part(dress)
    source_positions = accessor(source, source_bin, authored["attributes"]["POSITION"])
    source_uv = accessor(source, source_bin, authored["attributes"]["TEXCOORD_0"])
    faces = [
        source_face([source_positions[i] for i in triangle], [source_uv[i] for i in triangle])
        for triangle in triangles(source, source_bin, authored)
    ]
    # The alpha-cutout mesh uses the same source face order and a half-turn about Y.
    clipped_positions = [(-x, y, -z) for x, y, z in accessor(
        dress, dress_bin, clipped["attributes"]["POSITION"],
    )]
    result = [None] * len(clipped_positions)
    cursor = 0
    for triangle in triangles(dress, dress_bin, clipped):
        for index in range(cursor, len(faces)):
            coords = [face_uv(clipped_positions[i], faces[index]) for i in triangle]
            if any(uv is None or result[i] is not None and math.dist(uv, result[i]) > UV_TOLERANCE
                   for i, uv in zip(triangle, coords, strict=True)):
                continue
            for i, uv in zip(triangle, coords, strict=True):
                if result[i] is None:
                    result[i] = uv
            cursor = index
            break
        else:
            raise ValueError(f"no original garment face matches cutout triangle {triangle}")
    if any(uv is None for uv in result):
        raise ValueError("skirt contains an unreferenced vertex")
    return clipped, result


def write_uv(document, binary, primitive, coords):
    payload = b"".join(struct.pack("<2f", *uv) for uv in coords)
    attributes = primitive["attributes"]
    if "TEXCOORD_0" in attributes:
        entry = document["accessors"][attributes["TEXCOORD_0"]]
        view = document["bufferViews"][entry["bufferView"]]
        if view["byteLength"] != len(payload):
            raise ValueError("unexpected existing skirt UV buffer length")
        offset = view["byteOffset"]
        binary = binary[:offset] + payload + binary[offset + len(payload):]
    else:
        document["bufferViews"].append({
            "buffer": 0, "byteOffset": len(binary), "byteLength": len(payload), "target": 34962,
        })
        attributes["TEXCOORD_0"] = len(document["accessors"])
        document["accessors"].append({
            "bufferView": len(document["bufferViews"]) - 1,
            "componentType": 5126, "count": len(coords), "type": "VEC2",
        })
        binary += payload
        document["buffers"][0]["byteLength"] = len(binary)
    header = json.dumps(document, separators=(",", ":")).encode()
    header += b" " * (-len(header) % 4)
    return (
        struct.pack("<4sII", b"glTF", 2, 28 + len(header) + len(binary))
        + struct.pack("<I4s", len(header), b"JSON") + header
        + struct.pack("<I4s", len(binary), b"BIN\0") + binary
    )


def make_normal(source, binary):
    material = source["materials"][skirt_part(source)["material"]]
    texture = source["textures"][material["pbrMetallicRoughness"]["baseColorTexture"]["index"]]
    view = source["bufferViews"][source["images"][texture["source"]]["bufferView"]]
    start = view.get("byteOffset", 0)
    with Image.open(io.BytesIO(binary[start:start + view["byteLength"]])) as rgba:
        # Resizing LA uses premultiplied color, so transparent margins do not darken the cloth.
        small = rgba.convert("LA").resize((SIZE, SIZE), Image.Resampling.BOX)

    gray, alpha = small.split()
    heights = list(gray.tobytes())
    distances = [0 if value >= 24 else -1 for value in alpha.tobytes()]
    pending = deque(i for i, distance in enumerate(distances) if distance == 0)
    if not pending:
        raise ValueError("source skirt texture is transparent")
    # Extend the nearest cloth height across the alpha boundary for filtering and central differences.
    while pending:
        index = pending.popleft()
        x, y = index % SIZE, index // SIZE
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < SIZE and 0 <= ny < SIZE:
                neighbor = ny * SIZE + nx
                if distances[neighbor] == -1:
                    heights[neighbor] = heights[index]
                    distances[neighbor] = distances[index] + 1
                    pending.append(neighbor)

    height = Image.new("L", (SIZE, SIZE))
    height.putdata(heights)
    smooth = height.filter(ImageFilter.GaussianBlur(0.7)).load()
    result = Image.new("RGB", (SIZE, SIZE), (128, 128, 255))
    pixels = result.load()
    for index, distance in enumerate(distances):
        if distance <= 2:
            x, y = index % SIZE, index // SIZE
            dx = smooth[max(0, x - 1), y] - smooth[min(SIZE - 1, x + 1), y]
            dy = smooth[x, max(0, y - 1)] - smooth[x, min(SIZE - 1, y + 1)]
            vector = dx * SLOPE_STRENGTH / 255.0, dy * SLOPE_STRENGTH / 255.0, 1.0
            length = math.hypot(*vector)
            pixels[x, y] = tuple(round(127.5 * (1.0 + axis / length)) for axis in vector)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("avatar_vrm", type=Path)
    parser.add_argument("--dress", type=Path, default=ASSETS / "AvatarSample_B-pink-skirt.glb")
    parser.add_argument("--normal", type=Path, default=ASSETS / "AvatarSample_B-pink-skirt-normal.png")
    args = parser.parse_args()
    source, source_bin = read_glb(args.avatar_vrm)
    garment, garment_bin = read_glb(args.dress)
    primitive, coords = restore_uv(source, source_bin, garment, garment_bin)
    skirt = write_uv(garment, garment_bin, primitive, coords)
    normals = make_normal(source, source_bin)
    args.dress.write_bytes(skirt)
    normals.save(args.normal, optimize=True)


if __name__ == "__main__":
    main()
