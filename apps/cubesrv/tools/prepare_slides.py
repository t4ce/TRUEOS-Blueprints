#!/usr/bin/env python3
"""Prepare the checked-in slideshow from PNG/JPEG files (requires Pillow)."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageOps, ImageStat

SIZE = 512
DEFAULTS = [
    'd4c1829a-4abb-4247-a4a1-3e0171482de7.png',
    'HorizonServer.png', 'updlive/noway.png', 'updlive/nway.png',
    'updlive/0way.png', 'vid/demo_yelly_first_frame.png',
    'vid/Buro4K.jpeg', 'vid/YellyFHD.jpg',
    'vid/Photo from 2026-04-26 02-00-42.935475.jpeg',
    'vid/IMG_20260426_020424.jpg',
]

def prepare(source):
    with Image.open(source) as original:
        rgba = ImageOps.exif_transpose(original).convert('RGBA')
        # Ignore invisible RGB when calculating the padding/compositing color.
        mean = ImageStat.Stat(rgba.convert('RGB'), rgba.getchannel('A')).mean
        average = tuple(round(v) for v in mean)
        image = Image.new('RGB', rgba.size, average)
        image.paste(rgba, mask=rgba.getchannel('A'))
    if min(image.size) >= SIZE:
        return ImageOps.fit(image, (SIZE, SIZE), method=Image.Resampling.LANCZOS)
    image.thumbnail((SIZE, SIZE), Image.Resampling.LANCZOS)
    result = Image.new('RGB', (SIZE, SIZE), average)
    result.paste(image, ((SIZE-image.width)//2, (SIZE-image.height)//2))
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, default=Path(__file__).resolve().parents[4] / 'TRUEOS/tools')
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[1] / 'slides')
    parser.add_argument('images', nargs='*', help='Exactly ten PNG/JPEG paths relative to source root')
    args = parser.parse_args()
    sources = args.images or DEFAULTS
    if len(sources) != 10:
        parser.error('exactly ten images are required')
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = []
    for index, relative in enumerate(sources, 1):
        path = args.source_root / relative
        if path.suffix.lower() not in ('.png', '.jpg', '.jpeg'):
            parser.error(f'not a PNG/JPEG: {path}')
        image = prepare(path)
        name = f'{index:02}'
        image.save(args.output / f'{name}.png')
        (args.output / f'{name}.rgb').write_bytes(image.tobytes())
        manifest.append({'slide': index, 'source': relative, 'width': SIZE, 'height': SIZE})
    (args.output / 'sources.json').write_text(json.dumps(manifest, indent=2) + '\n')

if __name__ == '__main__':
    main()
