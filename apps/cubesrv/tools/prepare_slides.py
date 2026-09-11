#!/usr/bin/env python3
"""Prepare the checked-in slideshow from PNG/JPEG files (requires Pillow)."""
import argparse
import json
import os
import tempfile
from pathlib import Path
from PIL import Image, ImageOps, ImageStat

SIZE = 512
EXTENSIONS = ('.png', '.jpg', '.jpeg', '.jgp')
SLIDES = Path(__file__).resolve().parents[1] / 'slides'
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
        alpha = rgba.getchannel('A')
        mean = ImageStat.Stat(rgba.convert('RGB'), alpha).mean if alpha.getbbox() else (0, 0, 0)
        average = tuple(round(v) for v in mean)
        image = Image.new('RGB', rgba.size, average)
        image.paste(rgba, mask=rgba.getchannel('A'))
    if min(image.size) >= SIZE:
        return ImageOps.fit(image, (SIZE, SIZE), method=Image.Resampling.LANCZOS)
    image.thumbnail((SIZE, SIZE), Image.Resampling.LANCZOS)
    result = Image.new('RGB', (SIZE, SIZE), average)
    result.paste(image, ((SIZE-image.width)//2, (SIZE-image.height)//2))
    return result

def slide_paths(directory):
    return sorted(p for p in directory.iterdir() if p.is_file() and p.suffix.lower() in EXTENSIONS)


def normalize(path):
    """Normalize one existing slide, keeping its filename and standard encoding.

    Already-normalized files are left byte-for-byte intact, including JPEGs.
    Writes replace the original atomically after encoding completes.
    """
    path = Path(path)
    if path.suffix.lower() not in EXTENSIONS:
        raise ValueError(f'not a PNG/JPEG filename: {path}')
    target_format = 'PNG' if path.suffix.lower() == '.png' else 'JPEG'
    with Image.open(path) as source:
        source.load()
        if (source.size == (SIZE, SIZE) and source.mode == 'RGB'
                and source.getexif().get(274, 1) == 1
                and source.format == target_format
                and 'transparency' not in source.info):
            return False
    image = prepare(path)
    descriptor, temporary = tempfile.mkstemp(prefix=f'.{path.name}.', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as output:
            if target_format == 'JPEG':
                image.save(output, format='JPEG', quality=95, subsampling=0, progressive=False)
            else:
                image.save(output, format='PNG', optimize=True)
        os.chmod(temporary, path.stat().st_mode & 0o777)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, default=Path(__file__).resolve().parents[4] / 'TRUEOS/tools')
    parser.add_argument('--output', type=Path, default=SLIDES)
    parser.add_argument('--in-place', action='store_true', help='Normalize the current output directory, preserving filenames and PNG/JPEG encoding')
    parser.add_argument('images', nargs='*', help='Exactly ten PNG/JPEG paths relative to source root')
    args = parser.parse_args()
    if args.in_place:
        if args.images:
            parser.error('--in-place takes its images from --output; omit positional images')
        paths = slide_paths(args.output)
        if not paths:
            parser.error(f'no PNG/JPEG slides in {args.output}')
        changed = 0
        for path in paths:
            with Image.open(path) as image:
                before = image.size
            if normalize(path):
                changed += 1
                print(f'{path.name}: {before[0]}x{before[1]} -> {SIZE}x{SIZE}')
        print(f'{len(paths)} slides checked, {changed} normalized')
        return
    sources = args.images or DEFAULTS
    if len(sources) != 10:
        parser.error('exactly ten images are required')
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = []
    for index, relative in enumerate(sources, 1):
        path = args.source_root / relative
        if path.suffix.lower() not in EXTENSIONS:
            parser.error(f'not a PNG/JPEG: {path}')
        image = prepare(path)
        name = f'{index:02}'
        image.save(args.output / f'{name}.png')
        manifest.append({'slide': index, 'source': relative, 'width': SIZE, 'height': SIZE})
    (args.output / 'sources.json').write_text(json.dumps(manifest, indent=2) + '\n')

if __name__ == '__main__':
    main()
