"""Remove the authored Short Hair but we generate split from AvatarSample_B."""

import argparse
import json
from pathlib import Path
import struct


HAIR_MATERIAL = "N00_000_Hair_00_HAIR_01 (Instance)"
# Matched by bind positions against the named Blender object.
TRIANGLE_RANGES = (range(1501, 1749), range(4193, 4901))
VERTEX_RANGES = (range(1310, 1502), range(3451, 4000))
COMPONENT_FORMATS = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
COMPONENT_COUNTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def read_glb(raw):
    if struct.unpack_from("<4sII", raw) != (b"glTF", 2, len(raw)):
        raise ValueError("unexpected AvatarSample_B GLB header")
    size, chunk = struct.unpack_from("<I4s", raw, 12)
    document_end = 20 + size
    binary_size, binary_chunk = struct.unpack_from("<I4s", raw, document_end)
    binary = memoryview(raw)[document_end + 8:]
    if chunk != b"JSON" or binary_chunk != b"BIN\0" or len(binary) != binary_size:
        raise ValueError("expected a GLB with one embedded BIN chunk")
    return json.loads(raw[20:document_end]), binary


def read_accessor(document, binary, index):
    accessor = document["accessors"][index]
    view = document["bufferViews"][accessor["bufferView"]]
    layout = struct.Struct(
        "<" + COMPONENT_FORMATS[accessor["componentType"]] * COMPONENT_COUNTS[accessor["type"]]
    )
    if ("sparse" in accessor or accessor.get("byteOffset", 0) != 0
            or view.get("byteStride", layout.size) != layout.size
            or view["byteLength"] != accessor["count"] * layout.size):
        raise ValueError("expected a dedicated packed hair accessor")
    offset = view.get("byteOffset", 0)
    return accessor, layout, binary[offset:offset + view["byteLength"]]


def replace_accessor(replacements, accessor, layout, payload):
    view = accessor["bufferView"]
    if view in replacements:
        raise ValueError("unexpected shared hair buffer view")
    replacements[view] = payload
    accessor["count"] = len(payload) // layout.size
    for key, extrema in (("min", min), ("max", max)):
        if key in accessor:
            accessor[key] = [extrema(axis) for axis in zip(*layout.iter_unpack(payload))]


def kept_mapping(count, removed):
    mapping = [None] * count
    for new, old in enumerate(index for index in range(count) if index not in removed):
        mapping[old] = new
    return mapping


def prune(document, binary):
    (primitive,) = (
        primitive for mesh in document["meshes"] for primitive in mesh["primitives"]
        if document["materials"][primitive["material"]]["name"] == HAIR_MATERIAL
    )
    accessor, layout, indices = read_accessor(document, binary, primitive["indices"])
    corners = [index for (index,) in layout.iter_unpack(indices)]
    triangles = list(zip(corners[::3], corners[1::3], corners[2::3], strict=True))
    vertex_count = document["accessors"][primitive["attributes"]["POSITION"]]["count"]
    if len(triangles) != 11_590 or vertex_count != 9_263:
        raise ValueError("unexpected unpruned hair topology")

    removed_faces = {index for span in TRIANGLE_RANGES for index in span}
    retained = [triangle for index, triangle in enumerate(triangles) if index not in removed_faces]
    retained_vertices = set().union(*retained)
    split_vertices = {vertex for index in removed_faces for vertex in triangles[index]}
    removed_vertices = split_vertices - retained_vertices
    if removed_vertices != {index for span in VERTEX_RANGES for index in span}:
        raise ValueError("short-hair split does not match its authored vertex ranges")

    vertex_mapping = kept_mapping(vertex_count, removed_vertices)
    replacements = {}
    new_indices = b"".join(layout.pack(vertex_mapping[i]) for tri in retained for i in tri)
    replace_accessor(replacements, accessor, layout, new_indices)
    vertex_accessors = set(primitive["attributes"].values())
    for target in primitive.get("targets", []):
        vertex_accessors.update(target.values())
    for index in vertex_accessors:
        accessor, layout, rows = read_accessor(document, binary, index)
        if accessor["count"] != vertex_count:
            raise ValueError("inconsistent hair vertex count")
        retained_rows = b"".join(
            rows[i * layout.size:(i + 1) * layout.size]
            for i, mapped in enumerate(vertex_mapping) if mapped is not None
        )
        replace_accessor(replacements, accessor, layout, retained_rows)

    mapping = {
        "old_triangle_to_new": kept_mapping(len(triangles), removed_faces),
        "old_vertex_to_new": vertex_mapping,
    }
    return replacements, mapping


def write_glb(document, binary, replacements, output):
    output_binary = bytearray()
    for index, view in enumerate(document["bufferViews"]):
        offset = view.get("byteOffset", 0)
        payload = replacements.get(index, binary[offset:offset + view["byteLength"]])
        output_binary.extend(bytes(-len(output_binary) % 4))
        view.update(byteOffset=len(output_binary), byteLength=len(payload))
        output_binary.extend(payload)
    output_binary.extend(bytes(-len(output_binary) % 4))
    document["buffers"][0]["byteLength"] = len(output_binary)
    encoded = json.dumps(document, separators=(",", ":")).encode()
    encoded += b" " * (-len(encoded) % 4)
    output.write_bytes(
        struct.pack("<4sII", b"glTF", 2, 28 + len(encoded) + len(output_binary))
        + struct.pack("<I4s", len(encoded), b"JSON") + encoded
        + struct.pack("<I4s", len(output_binary), b"BIN\0") + output_binary
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="unpruned AvatarSample_B.glb")
    parser.add_argument("output", type=Path)
    parser.add_argument("--mapping", type=Path, help="write the old-to-new index maps as JSON")
    args = parser.parse_args()
    document, binary = read_glb(args.source.read_bytes())
    replacements, mapping = prune(document, binary)
    write_glb(document, binary, replacements, args.output)
    if args.mapping:
        args.mapping.write_text(json.dumps(mapping, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
