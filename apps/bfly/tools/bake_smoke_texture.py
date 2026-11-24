#!/usr/bin/env python3
"""Bake either bfly smoke source into tightly packed R8 rows."""

import argparse
from pathlib import Path

from PIL import Image, ImageChops


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cloud-source", type=Path, help="Bake the RGBA smoke cloud instead of vfx.jpg")
    args = parser.parse_args()
    app = Path(__file__).resolve().parents[1]
    output = app / "shaders/trueos"

    if args.cloud_source is not None:
        with Image.open(args.cloud_source) as source:
            if source.size != (1254, 1254) or source.mode != "RGBA":
                raise ValueError("Expected the 1254×1254 RGBA smoke cloud")
            # Center the 1024×1024 crop on the visible cloud with a transparent margin.
            cloud = source.crop((116, 112, 1140, 1136))
            density = ImageChops.multiply(cloud.convert("L"), cloud.getchannel("A"))
        texture = density.resize((512, 512), Image.Resampling.LANCZOS)
        (output / "bfly-cloud.r8").write_bytes(texture.tobytes())
        return

    with Image.open(app / "tools/vfx.jpg") as source:
        if source.size != (860, 240):
            raise ValueError("Expected the 860×240 side-by-side smoke source")
        dark_half = source.crop((0, 0, 430, 240)).convert("L")

    # The rotation puts the thin end above the dense plume in top-down rows.
    portrait = dark_half.transpose(Image.Transpose.ROTATE_90)
    # Keep the center strip, including the soft sides of the plume. The crop
    # also clears the JPEG's center seam and matches the output aspect ratio.
    center_strip = portrait.crop((20, 20, 220, 420))
    texture = center_strip.resize((64, 128), Image.Resampling.LANCZOS)
    (output / "bfly-smoke.r8").write_bytes(texture.tobytes())


if __name__ == "__main__":
    main()
