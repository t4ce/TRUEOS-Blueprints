"""Pack NASA/Goddard/SDO's Sun Emits a Solstice CME for UI4 scanout."""

from math import hypot
from pathlib import Path

from PIL import Image


SIDE = 512
TOOLS = Path(__file__).resolve().parent
SOURCE = TOOLS / "GSFC_20171208_Archive_e001435~large.jpg"
DESTINATION = TOOLS.parent / "shaders" / "trueos" / "bfly-sun.rgba8"


def smoothstep(low, high, value):
    t = min(1.0, max(0.0, (value - low) / (high - low)))
    return t * t * (3 - 2 * t)


def main():
    with Image.open(SOURCE) as original:
        image = original.convert("RGB").resize((SIDE, SIDE), Image.Resampling.LANCZOS)
    rgb_bytes = image.tobytes()
    packed = bytearray()
    center = (SIDE - 1) / 2
    for offset in range(SIDE * SIDE):
        rgb = rgb_bytes[offset * 3 : (offset + 1) * 3]
        y, x = divmod(offset, SIDE)
        radius = hypot(x - center, y - center)
        disk = 1 - smoothstep(195, 205, radius)
        # Preserve the disk and the corona on black. Suppress very dim JPEG
        # background residue outside the limb when keyed over brighter colors.
        strength = max(disk, smoothstep(2, 14, max(rgb)))
        red, green, blue = (round(value * strength) for value in rgb)
        alpha = max(round(255 * disk), red, green, blue)
        gray = (299 * red + 587 * green + 114 * blue + 500) // 1000
        packed.extend((gray, gray, gray, alpha))
    DESTINATION.write_bytes(packed)


if __name__ == "__main__":
    main()
