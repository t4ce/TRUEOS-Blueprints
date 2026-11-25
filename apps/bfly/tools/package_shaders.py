#!/usr/bin/env python3
"""Verify bfly's native shaders and rebuild them from pinned Intel compiles."""

import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import tarfile


APP = Path(__file__).resolve().parents[1]
NATIVE = APP / "shaders/trueos"
KERNEL = APP.parents[2] / "TRUEOS"
MAGIC = b"APSHDR1\0"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def captured_package(record, capture):
    directory = capture / record["capture_directory"] / record.get("stage_directory", "stock-bin")
    payload = bytearray(struct.pack("<I", len(record["stages"])))
    for stage in record["stages"]:
        path = (NATIVE / stage["asset_file"] if "asset_file" in stage
                else directory / stage["capture_file"])
        data = path.read_bytes()
        size = stage["bytes"]
        require(data[size:].hex() == stage.get("capture_tail", ""), "Unexpected capture padding")
        payload.extend(struct.pack("<II", stage["kind"], size))
        payload.extend(data[:size])
    return MAGIC + struct.pack("<II", record["id"], len(payload)) + payload


def verify_package(record, package):
    name = record["name"]
    require(len(package) == record["bytes"] and digest(package) == record["sha256"],
            f"Package digest or size mismatch for {name}")
    expected_header = (record["id"], len(package) - 16, len(record["stages"]))
    require(package[:8] == MAGIC and struct.unpack_from("<III", package, 8) == expected_header,
            f"Invalid package header for {name}")
    stages = []
    offset = 20
    for stage in record["stages"]:
        kind, size = struct.unpack_from("<II", package, offset)
        require((kind, size) == (stage["kind"], stage["bytes"]), f"Invalid stage in {name}")
        offset += 8
        data = package[offset:offset + size]
        require(digest(data) == stage["sha256"],
                f"Stage digest mismatch for {name} {stage['name']}")
        stages.append((stage["name"], data))
        offset += size
    require(offset == len(package), f"Unexpected trailing data in {name}")
    return stages


def verify_compiler_capture(record, stages, capture, root):
    cache_path = root / capture["directory"] / "pipeline_cache.bin"
    cache = cache_path.read_bytes()
    require(digest(cache) == capture["pipeline_cache_sha256"],
            f"Pipeline cache digest mismatch for {record['name']}")
    vendor = int(capture["pci_vendor"], 16)
    device = int(capture["pci_device"], 16)
    require(struct.unpack_from("<IIII", cache) == (32, 1, vendor, device),
            f"Unexpected Vulkan pipeline cache header in {cache_path}")
    offsets = (capture["stage_offsets"] if "stage_offsets" in capture
               else record["offline_compiles"]["stage_offsets"])
    for metadata, (name, data) in zip(record["stages"], stages, strict=True):
        offset = offsets[name]
        captured_data = data + bytes.fromhex(metadata.get("capture_tail", ""))
        require(cache[offset:offset + len(captured_data)] == captured_data,
                f"Compiled ISA differs for {record['name']} {name} on {device:04x}")


def blueprint_shader_section(path):
    image = path.read_bytes()
    if image[:4] == b"TRBP":
        kind = struct.unpack_from("<H", image, 6)[0] & 255
        size = struct.unpack_from("<I", image, 16)[0]
        image = image[24:24 + size]
        if kind == 3:
            raw = subprocess.run(["lz4", "-d", "-c"], input=image,
                                 capture_output=True, check=True).stdout
            with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
                image = archive.extractfile("module.elf").read()
        else:
            require(kind == 1, f"Unsupported Blueprint payload in {path}")

    header = struct.unpack_from("<16sHHIQQQIHHHHHH", image)
    require(header[0][:6] == b"\x7fELF\x02\x01", f"Expected little-endian ELF64 in {path}")
    table, stride, count, names_index = header[6], header[11], header[12], header[13]
    require(stride == 64, f"Unexpected ELF section size in {path}")

    def section(index):
        return struct.unpack_from("<IIQQQQIIQQ", image, table + stride * index)

    names_header = section(names_index)
    names = image[names_header[4]:names_header[4] + names_header[5]]
    shader_data = []
    for index in range(count):
        name_offset, _, flags, _, start, size, *_ = section(index)
        if names[name_offset:].split(b"\0", 1)[0] == b".trueos.shaders":
            require(flags & 2 == 0, f"Guest-allocated shader metadata in {path}")
            shader_data.append(image[start:start + size])
    require(len(shader_data) == 1, f"Expected one .trueos.shaders section in {path}")
    return shader_data[0]


def check_publication(record, trust, kernel_image, rebuild_from, *, archived=False):
    if not archived:
        for name, expected in record["sources"].items():
            require(digest((APP / "shaders" / name).read_bytes()) == expected,
                    f"Shader source differs from native capture for {name}")
    path = NATIVE / (record["name"] + ".apshader")
    package = captured_package(record, rebuild_from) if rebuild_from else path.read_bytes()
    stages = verify_package(record, package)
    for metadata, (name, data) in zip(record["stages"], stages, strict=True):
        if "asset_file" in metadata:
            require((NATIVE / metadata["asset_file"]).read_bytes() == data,
                    f"Embedded asset differs for {record['name']} {name}")
    sha256 = bytes.fromhex(record["sha256"])
    if kernel_image is not None:
        require(sha256 in kernel_image, f"{record['name']} trust digest missing from kernel")
        require(package not in kernel_image, f"{record['name']} package linked in kernel")
        for name, stage in stages:
            require(stage not in kernel_image, f"{record['name']} {name} stage linked in kernel")
    signature = ", ".join(f"0x{byte:02x}" for byte in sha256)
    require(f"({record['id']}, {len(package)}, [{signature}])" in trust,
            f"Kernel trust record does not match {record['name']}")
    return path, package, stages


def check_graphics_publication(record, trust, kernel_image, rebuild_from):
    path, package, stages = check_publication(record, trust, kernel_image, rebuild_from)
    if rebuild_from:
        for capture in record["offline_compiles"]["targets"]:
            verify_compiler_capture(record, stages, capture, rebuild_from)
        path.write_bytes(package)
    return package


def check_history_publication(record, trust, kernel_image, rebuild_from):
    path, package, _ = check_publication(record, trust, kernel_image, rebuild_from)
    if rebuild_from:
        for capture in record["offline_compiles"]["targets"]:
            directory = capture["directory"]
            target_record = {**record, "capture_directory": directory}
            require(captured_package(target_record, rebuild_from) == package,
                    f"Compiled history package differs for target {directory}")
            stage = record["stages"][0]["name"]
            spirv = (rebuild_from / directory / (stage + ".spv")).read_bytes()
            require(digest(spirv) == record["offline_compiles"]["spirv_sha256"],
                    f"Compiled history SPIR-V differs for target {directory}")
        path.write_bytes(package)
    return package


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, default=KERNEL)
    parser.add_argument("--kernel-elf", type=Path)
    parser.add_argument("--blueprint", type=Path, help="check the shader packages in the Blueprint")
    parser.add_argument("--rebuild-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the archived texture demo bundle")
    parser.add_argument("--rebuild-direct-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the archived single-design tessellation stages from their native captures")
    parser.add_argument("--rebuild-mixed-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the mixed flock tessellation stages from their native captures")
    parser.add_argument("--rebuild-tier-widths-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the tiered line-width tessellation stages from their native captures")
    parser.add_argument("--rebuild-fill-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the original lobe-fill shader from its native captures")
    parser.add_argument("--rebuild-inset-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the inset wing-fill shader from its native captures")
    parser.add_argument("--rebuild-breed-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the seeded wing-fill shader from its native captures")
    parser.add_argument("--rebuild-translucent-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the translucent wing-fill shader from its native captures")
    parser.add_argument("--rebuild-inset-outline-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the outline over the inset wing fill from its native captures")
    parser.add_argument("--rebuild-history-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild and compare the earlier monotone history kernel against its IGC compiles")
    parser.add_argument("--rebuild-smoke-history-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the textured history package from its IGC compiles and embedded R8")
    parser.add_argument("--rebuild-cloud-history-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the cloud history package from its IGC compiles and embedded R8")
    parser.add_argument("--rebuild-cloud-trail-from", type=Path, metavar="CAPTURE_DIR",
                        help="rebuild the higher-contrast cloud history shader from its IGC compiles")
    parser.add_argument("--verify-offline-from", type=Path, metavar="CAPTURE_DIR",
                        help="compare the archived texture demo against its pinned offline Vulkan pipeline caches")
    args = parser.parse_args()
    records = json.loads((NATIVE / "manifest.json").read_text())
    trust = " ".join((args.kernel / "src/gpu/app_shader_trust.rs").read_text().split())
    kernel_image = args.kernel_elf.read_bytes() if args.kernel_elf else None
    packages = {}
    for record in records:
        path, package, stages = check_publication(record, trust, kernel_image, args.rebuild_from)
        if args.verify_offline_from:
            for capture in record["offline_compiles"]["targets"]:
                verify_compiler_capture(record, stages, capture, args.verify_offline_from)
        packages[path] = package
    direct_record = json.loads((NATIVE / "direct-wings-manifest.json").read_text())
    check_graphics_publication(
        direct_record, trust, kernel_image, args.rebuild_direct_from)
    mixed_record = json.loads((NATIVE / "mixed-wings-manifest.json").read_text())
    check_graphics_publication(
        mixed_record, trust, kernel_image, args.rebuild_mixed_from)
    widths_record = json.loads((NATIVE / "tier-width-wings-manifest.json").read_text())
    widths_package = check_graphics_publication(
        widths_record, trust, kernel_image, args.rebuild_tier_widths_from)
    fill_record = json.loads((NATIVE / "filled-wings-manifest.json").read_text())
    check_graphics_publication(
        fill_record, trust, kernel_image, args.rebuild_fill_from)
    inset_record = json.loads((NATIVE / "inset-wings-manifest.json").read_text())
    check_graphics_publication(inset_record, trust, kernel_image, args.rebuild_inset_from)
    breed_record = json.loads((NATIVE / "breed-wings-manifest.json").read_text())
    check_graphics_publication(breed_record, trust, kernel_image, args.rebuild_breed_from)
    translucent_record = json.loads((NATIVE / "translucent-wings-manifest.json").read_text())
    translucent_package = check_graphics_publication(
        translucent_record, trust, kernel_image, args.rebuild_translucent_from)
    inset_outline_record = json.loads((NATIVE / "inset-outline-manifest.json").read_text())
    inset_outline_package = check_graphics_publication(
        inset_outline_record, trust, kernel_image, args.rebuild_inset_outline_from)
    celestial_record = json.loads((NATIVE / "celestial-masks-manifest.json").read_text())
    _, celestial_masks, _ = check_publication(celestial_record, trust, kernel_image, NATIVE)
    history_record = json.loads((NATIVE / "history-manifest.json").read_text())
    history_package = check_history_publication(
        history_record, trust, kernel_image, args.rebuild_history_from)
    # The untextured capture predates the current outline shaders.
    untextured_record = json.loads((NATIVE / "untextured-wings-manifest.json").read_text())
    check_publication(untextured_record, trust, kernel_image, None, archived=True)
    smoke_record = json.loads((NATIVE / "smoke-history-manifest.json").read_text())
    smoke_package = check_history_publication(
        smoke_record, trust, kernel_image, args.rebuild_smoke_history_from)
    cloud_record = json.loads((NATIVE / "cloud-history-manifest.json").read_text())
    cloud_package = check_history_publication(
        cloud_record, trust, kernel_image, args.rebuild_cloud_history_from)
    trail_record = json.loads((NATIVE / "cloud-trail-manifest.json").read_text())
    trail_package = check_history_publication(
        trail_record, trust, kernel_image, args.rebuild_cloud_trail_from)
    bundle = b"".join(packages.values())
    bundle_file = NATIVE / "app-shaders.bundle"
    if args.rebuild_from:
        for path, package in packages.items():
            path.write_bytes(package)
        bundle_file.write_bytes(bundle)
    require(bundle_file.read_bytes() == bundle, "bfly bundle differs from its packages")
    if args.blueprint:
        expected = b"".join((widths_package, smoke_package, cloud_package, trail_package,
                             history_package, translucent_package, inset_outline_package,
                             celestial_masks))
        require(blueprint_shader_section(args.blueprint) == expected,
                f"Shader metadata differs from bfly's wings and history packages in {args.blueprint}")
    print("Verified bfly native shader sources, captured stages, and kernel SHA-256 records")
    if args.verify_offline_from:
        print("Verified texture shader ISA in the offline ADL-S and RPL-S pipeline caches")
    if args.rebuild_direct_from:
        print("Verified direct tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_mixed_from:
        print("Verified mixed flock tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_tier_widths_from:
        print("Verified tiered line-width tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_fill_from:
        print("Verified filled-wing tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_inset_from:
        print("Verified inset wing-fill tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_breed_from:
        print("Verified seeded wing-fill tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_translucent_from:
        print("Verified translucent wing-fill tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_inset_outline_from:
        print("Verified inset wing-outline tessellation ISA in the offline Vulkan pipeline caches")
    if args.rebuild_history_from:
        print("Verified outline history Zebin and SPIR-V for ADL-S and RPL-S")
    if args.rebuild_smoke_history_from:
        print("Verified textured history Zebin and SPIR-V for ADL-S and RPL-S, plus embedded R8")
    if args.rebuild_cloud_history_from:
        print("Verified cloud history Zebin and SPIR-V for ADL-S and RPL-S, plus embedded R8")
    if args.rebuild_cloud_trail_from:
        print("Verified higher-contrast cloud history Zebin and SPIR-V for ADL-S and RPL-S")


if __name__ == "__main__":
    main()
