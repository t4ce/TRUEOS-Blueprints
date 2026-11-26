"""Pack the resident wake RGB tiles and shared tear alpha."""

from pathlib import Path

from PIL import Image


TOOLS = Path(__file__).resolve().parent
SIZE = (256, 256)
SOURCES = (
    TOOLS / "dwn/600px-Sunrise_background.jpg",
    TOOLS / "dwn/600px-Twilight_background.png",
    TOOLS / "uni.jpg",
)
TEAR_MASK = TOOLS / "twilight-lab/wake-tear.r8"
DESTINATION = TOOLS.parent / "shaders/trueos/bfly-sunrise.rgba8"


def wake_atlas():
    tear = TEAR_MASK.read_bytes()
    if len(tear) != SIZE[0] * SIZE[1]:
        raise ValueError("Unexpected wake tear mask size")
    tiles = []
    for row, source in enumerate(SOURCES):
        with Image.open(source) as image:
            tile = image.convert("RGB").resize(SIZE, Image.Resampling.LANCZOS).convert("RGBA")
        if row == 0:
            tile.putalpha(Image.frombytes("L", SIZE, tear))
        tiles.append(tile.tobytes())
    return b"".join(tiles)


if __name__ == "__main__":
    DESTINATION.write_bytes(wake_atlas())
