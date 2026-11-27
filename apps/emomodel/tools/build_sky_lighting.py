"""Bake the EmoModel native sky lookup from the 2K WebGL lighting study."""

import base64
import json
import math
import struct
from pathlib import Path


APP = Path(__file__).resolve().parents[1]
SOURCE = APP / "studies/sky/skybox-lighting.json"
OUTPUT = APP / "assets/sky-lighting.bin"
REFLECTION_SIZE = 65
LINEAR_RGB_STEPS = 4096


def octahedral_direction(x, y):
    """Decode a Y-up octahedral grid point into the study's world space."""
    last = REFLECTION_SIZE - 1
    u, v = 2 * x / last - 1, 2 * y / last - 1
    up = 1 - abs(u) - abs(v)
    fold = max(0.0, -up)
    world_x = u - math.copysign(fold, u)
    world_z = v - math.copysign(fold, v)
    length = math.hypot(world_x, up, world_z)
    return world_x / length, up / length, world_z / length


def lerp(a, b, mix):
    return a + (b - a) * mix


def sample_reflection(pixels, width, height, direction):
    """Match the study's linear filter, wrapping longitude and clamping latitude."""
    x, y, z = direction
    phi = math.atan2(x, -z) if x * x + z * z > 1e-8 else 0.0
    theta = math.acos(min(1.0, max(-1.0, y)))
    u = (phi / math.tau + 0.5) * width - 0.5
    v = theta / math.pi * height - 0.5
    i, j = math.floor(u), math.floor(v)
    mix_x, mix_y = u - i, v - j

    left, right = i % width, (i + 1) % width
    top = min(height - 1, max(0, j))
    bottom = min(height - 1, max(0, j + 1))
    northwest = (top * width + left) * 4
    northeast = (top * width + right) * 4
    southwest = (bottom * width + left) * 4
    southeast = (bottom * width + right) * 4
    return bytes(
        round(lerp(lerp(pixels[northwest + c], pixels[northeast + c], mix_x),
                   lerp(pixels[southwest + c], pixels[southeast + c], mix_x), mix_y))
        for c in range(3)
    )


def check_seams(reflection):
    """The mirrored halves of each outside edge represent the same directions."""
    last = REFLECTION_SIZE - 1

    def pixel(x, y):
        offset = (y * REFLECTION_SIZE + x) * 3
        return reflection[offset:offset + 3]

    for i in range(REFLECTION_SIZE):
        for edge in (0, last):
            if (pixel(i, edge) != pixel(last - i, edge)
                    or pixel(edge, i) != pixel(edge, last - i)):
                raise ValueError("octahedral reflection edges do not match")


def encode_srgb(linear):
    display = 12.92 * linear if linear <= 0.0031308 else 1.055 * linear ** (1 / 2.4) - 0.055
    return round(255 * display)


def main():
    source = json.loads(SOURCE.read_bytes())
    harmonics = source["sh"]
    if len(harmonics) != 9 or any(len(rgb) != 3 for rgb in harmonics):
        raise ValueError("expected nine RGB irradiance coefficients")
    irradiance = struct.pack("<27f", *(channel for rgb in harmonics for channel in rgb))

    width, height = source["reflectionWidth"], source["reflectionHeight"]
    pixels = base64.b64decode(source["reflectionRgba"], validate=True)
    if width <= 0 or height <= 0 or len(pixels) != width * height * 4:
        raise ValueError("invalid reflection texture dimensions")
    reflection = b"".join(
        sample_reflection(pixels, width, height, octahedral_direction(x, y))
        for y in range(REFLECTION_SIZE)
        for x in range(REFLECTION_SIZE)
    )
    check_seams(reflection)

    srgb = bytes(encode_srgb(i / LINEAR_RGB_STEPS) for i in range(LINEAR_RGB_STEPS + 1))
    OUTPUT.write_bytes(irradiance + reflection + srgb)


if __name__ == "__main__":
    main()
