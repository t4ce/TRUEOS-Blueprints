"""Extract the static fixture and texture crops from the original AvatarSample_B VRM."""

import argparse
import copy
import hashlib
import io
import json
import math
from pathlib import Path
import struct

from PIL import Image


SOURCE_SHA256 = "7fca4a77fdc60ab2c78a9907430744562626180125fa386eb74fb2ea15c2e518"
BACK_HAIR_TRIANGLES = ((1770, 4194), (6013, 11669))
PADDING = 8


def accessor(document, blob, index, code, components):
    entry = document["accessors"][index]
    view = document["bufferViews"][entry["bufferView"]]
    data_format = "<" + code * components
    size = struct.calcsize(data_format)
    if view.get("byteStride", size) != size:
        raise ValueError("unexpected interleaved accessor in AvatarSample_B")
    offset = view.get("byteOffset", 0) + entry.get("byteOffset", 0)
    return list(struct.iter_unpack(data_format, blob[offset : offset + entry["count"] * size]))


def texture_crop(document, blob, source, uvs, preserve_resolution):
    image = document["images"][document["textures"][source]["source"]]
    view = document["bufferViews"][image["bufferView"]]
    offset = view.get("byteOffset", 0)
    source = Image.open(io.BytesIO(blob[offset : offset + view["byteLength"]]))
    width, height = source.size
    left = max(0, math.floor(min(u for u, _ in uvs) * width) - PADDING)
    top = max(0, math.floor(min(v for _, v in uvs) * height) - PADDING)
    right = min(width, math.ceil(max(u for u, _ in uvs) * width) + PADDING)
    bottom = min(height, math.ceil(max(v for _, v in uvs) * height) + PADDING)
    crop = source.convert("RGBA").crop((left, top, right, bottom))
    if not preserve_resolution:
        crop.thumbnail((2048, 2048), Image.Resampling.LANCZOS)
    encoded = io.BytesIO()
    crop.save(encoded, format="PNG", optimize=True)
    return encoded.getvalue(), (width, height, left, top, right - left, bottom - top)


class Glb:
    def __init__(self):
        self.data = bytearray()
        self.views = []
        self.accessors = []

    def view(self, contents, target=None):
        self.data.extend(bytes(-len(self.data) % 4))
        view = {"buffer": 0, "byteOffset": len(self.data), "byteLength": len(contents)}
        if target is not None:
            view["target"] = target
        self.views.append(view)
        self.data.extend(contents)
        return len(self.views) - 1

    def accessor(self, values, code, component_type, gltf_type, target, bounds=False):
        contents = struct.pack("<" + code * len(values), *values)
        self.accessors.append({
            "bufferView": self.view(contents, target),
            "componentType": component_type,
            "count": len(values) // {"SCALAR": 1, "VEC2": 2, "VEC3": 3}[gltf_type],
            "type": gltf_type,
        })
        if bounds:
            self.accessors[-1]["min"] = [min(values[i::3]) for i in range(3)]
            self.accessors[-1]["max"] = [max(values[i::3]) for i in range(3)]
        return len(self.accessors) - 1

    def primitive(self, surface, material, crop):
        width, height, left, top, span_x, span_y = crop
        uv = [
            value
            for u, v in surface["uv"]
            for value in ((u * width - left) / span_x, (v * height - top) / span_y)
        ]
        position = [value for point in surface["positions"] for value in point]
        normal = [value for point in surface["normals"] for value in point]
        return {
            "attributes": {
                "POSITION": self.accessor(position, "f", 5126, "VEC3", 34962, bounds=True),
                "NORMAL": self.accessor(normal, "f", 5126, "VEC3", 34962),
                "TEXCOORD_0": self.accessor(uv, "f", 5126, "VEC2", 34962),
            },
            "indices": self.accessor(surface["indices"], "H", 5123, "SCALAR", 34963),
            "material": material,
        }

    def save(self, document, output):
        self.data.extend(bytes(-len(self.data) % 4))
        document["accessors"] = self.accessors
        document["bufferViews"] = self.views
        document["buffers"] = [{"byteLength": len(self.data)}]
        encoded = json.dumps(document, separators=(",", ":")).encode()
        encoded += b" " * (-len(encoded) % 4)
        length = 28 + len(encoded) + len(self.data)
        output.write_bytes(
            struct.pack("<4sII", b"glTF", 2, length)
            + struct.pack("<I4s", len(encoded), b"JSON")
            + encoded
            + struct.pack("<I4s", len(self.data), b"BIN\x00")
            + self.data
        )


def extract_surfaces(document, blob):
    surfaces = []
    for mesh in document["meshes"]:
        for primitive in mesh["primitives"]:
            attrs = primitive["attributes"]
            positions = accessor(document, blob, attrs["POSITION"], "f", 3)
            normals = accessor(document, blob, attrs["NORMAL"], "f", 3)
            uv = accessor(document, blob, attrs["TEXCOORD_0"], "f", 2)
            indices = [i for (i,) in accessor(document, blob, primitive["indices"], "I", 1)]
            material = primitive["material"]
            if material == 6:
                # Ranges verified against the welded back locks in this source asset.
                (back_start, back_end), (last_start, last_end) = BACK_HAIR_TRIANGLES
                if len(indices) != last_end * 3:
                    raise ValueError("unexpected AvatarSample_B hair topology")
                partitions = [
                    ("Hair", 2, indices[: back_start * 3] + indices[back_end * 3 : last_start * 3]),
                    ("Back Hair", 3, indices[back_start * 3 : back_end * 3] + indices[last_start * 3 :]),
                ]
            else:
                group = 0 if material < 4 else 1
                partitions = [(document["materials"][material]["name"], group, indices)]
            for name, group, partition in partitions:
                vertices = sorted(set(partition))
                remap = {vertex: index for index, vertex in enumerate(vertices)}
                surfaces.append({
                    "name": name,
                    "material": material,
                    "texture_group": group,
                    "positions": [positions[i] for i in vertices],
                    "normals": [normals[i] for i in vertices],
                    "uv": [uv[i] for i in vertices],
                    "indices": [remap[i] for i in partition],
                })
    return surfaces


def prepare(source, output):
    raw = source.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA256:
        raise ValueError("expected the original textured AvatarSample_B v1.0 VRM")
    json_size = struct.unpack_from("<I", raw, 12)[0]
    original = json.loads(raw[20 : 20 + json_size])
    blob = memoryview(raw)[28 + json_size :]
    surfaces = extract_surfaces(original, blob)
    fixture = Glb()
    images = []
    crops = []
    texture_names = ["Face", "Body and shoes", "Hair", "Back Hair"]
    for group, name in enumerate(texture_names):
        uvs = [uv for surface in surfaces if surface["texture_group"] == group for uv in surface["uv"]]
        encoding, crop = texture_crop(
            original, blob, 0 if group == 0 else 3, uvs, preserve_resolution=group == 3
        )
        images.append({"name": name, "bufferView": fixture.view(encoding), "mimeType": "image/png"})
        crops.append(crop)

    materials = []
    avatar = []
    back_hair = []
    for surface in surfaces:
        group = surface["texture_group"]
        original_material = original["materials"][surface["material"]]
        material = copy.deepcopy({
            key: original_material[key]
            for key in ("alphaMode", "alphaCutoff", "doubleSided", "pbrMetallicRoughness", "extensions")
            if key in original_material
        })
        material["name"] = surface["name"]
        material["pbrMetallicRoughness"]["baseColorTexture"] = {"index": group}
        part = fixture.primitive(surface, len(materials), crops[group])
        materials.append(material)
        if group == 3:
            back_hair.append(part)
        else:
            avatar.append(part)

    metadata = original["extensions"]["VRM"]["meta"]
    fixture.save({
        "asset": {
            "version": "2.0",
            "generator": "TRUEOS Hair asset preparation",
            "copyright": "AvatarSample_B by VRoid Project",
            "extras": {"sourceSha256": SOURCE_SHA256, "license": metadata["otherLicenseUrl"]},
        },
        "extensionsUsed": ["KHR_materials_unlit"],
        "scene": 0,
        "scenes": [{"nodes": [0, 1]}],
        "nodes": [{"name": "Avatar", "mesh": 0}, {"name": "Back Hair", "mesh": 1}],
        "meshes": [{"name": "Avatar", "primitives": avatar}, {"name": "Back Hair", "primitives": back_hair}],
        "materials": materials,
        "images": images,
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 33071, "wrapT": 33071}],
        "textures": [{"sampler": 0, "source": i} for i in range(len(images))],
    }, output)
    print(f"Wrote {output} ({output.stat().st_size:,} bytes)")
    print(f"Back Hair atlas crop: {crops[3][2:]}; long triangles: {len(surfaces[-1]['indices']) // 3}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="original textured AvatarSample_B VRM")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[1] / "assets/avatar.glb")
    args = parser.parse_args()
    prepare(args.source, args.output)


if __name__ == "__main__":
    main()
