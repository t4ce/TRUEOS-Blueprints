#!/usr/bin/env python3
"""Bake six tiered CubeImage slabs into one PNG atlas (Pillow, offline only)."""
import argparse
import hashlib
import io
import json
import math
import os
import tempfile
from pathlib import Path
from PIL import Image, ImageOps

SLIDES = Path(__file__).resolve().parents[1] / 'slides'
# source resolution, 1 x N x N assembly, projected texture pixels per slab face.
TIERS = {'tier1': (48, 6, 6), 'tier2': (128, 8, 16), 'tier3': (256, 16, 32)}
EXTENSIONS = ('.png', '.jpg', '.jpeg', '.jgp')


def prepare(source, size='tier3'):
    """Center crop oversized axes and black-pad undersized axes, without scaling."""
    side = TIERS[size][0]
    with Image.open(source) as original:
        rgba = ImageOps.exif_transpose(original).convert('RGBA')
        width, height = min(side, rgba.width), min(side, rgba.height)
        sx, sy = max(0, (rgba.width-side)//2), max(0, (rgba.height-side)//2)
        crop = rgba.crop((sx, sy, sx+width, sy+height))
        image = Image.new('RGB', (side, side), (0, 0, 0))
        image.paste(crop, ((side-width)//2, (side-height)//2), crop.getchannel('A'))
        return image


def texture(image, size):
    """Exact grid-center sampling and fixed exposure 1.05 / palette 16."""
    pixels = TIERS[size][2]
    sampled = Image.new('RGB', (pixels, pixels))
    sampled.putdata([image.getpixel((int((x+.5)*image.width/pixels),
                                    int((y+.5)*image.height/pixels)))
                     for y in range(pixels) for x in range(pixels)])
    return sampled.point([math.floor(min(1, v/255*1.05)*15+.5)*17 for v in range(256)]*3)


def load_manifest(path, faces=None):
    entries = json.loads(path.read_text())
    if not isinstance(entries, list) or len(entries) < 6:
        raise ValueError('sources.json must contain at least six images')
    ids = set()
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {'slide', 'source', 'Size'}:
            raise ValueError('each source requires exactly slide, source and Size')
        if type(entry['slide']) is not int or entry['slide'] < 1 or entry['slide'] in ids:
            raise ValueError('slide IDs must be unique positive integers')
        ids.add(entry['slide'])
        if not isinstance(entry['Size'], str) or entry['Size'] not in TIERS or not isinstance(entry['source'], str):
            raise ValueError('Size must be tier1, tier2 or tier3')
        if Path(entry['source']).suffix.lower() not in EXTENSIONS:
            raise ValueError('source must be a PNG/JPEG filename')
    by_id = {entry['slide']: entry for entry in entries}
    selected = faces if faces is not None else [entry['slide'] for entry in entries[:6]]
    if len(set(selected)) != 6 or any(i not in by_id for i in selected):
        raise ValueError('select six different existing slide IDs')
    return [by_id[i] for i in selected]


def bake(entries, source_root):
    tile = max(TIERS[e['Size']][2] for e in entries)+2
    atlas = Image.new('RGB', (tile*3, tile*2))
    for face, entry in enumerate(entries):
        image = texture(prepare(source_root / entry['source'], entry['Size']), entry['Size'])
        n = image.width
        # Clamp apron, including corners. Unused portions of small tier tiles are black.
        padded = Image.new('RGB', (n+2, n+2))
        padded.paste(image, (1, 1))
        for x in range(n+2):
            for y in (0, n+1): padded.putpixel((x,y), image.getpixel((min(n-1,max(0,x-1)), min(n-1,max(0,y-1)))))
        for y in range(1,n+1):
            padded.putpixel((0,y),image.getpixel((0,y-1)))
            padded.putpixel((n+1,y),image.getpixel((n-1,y-1)))
        atlas.paste(padded, (face%3*tile, face//3*tile))
    png = io.BytesIO()
    atlas.save(png, format='PNG', optimize=True)
    package = b'CGA1' + bytes([4, 6] + [int(e['Size'][-1]) for e in entries]) + bytes(4) + png.getvalue()
    if len(package) > 4*1024*1024: raise ValueError('gallery exceeds transfer budget')
    return package


def atomic_write(path, data):
    fd, temporary = tempfile.mkstemp(prefix='.'+path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as output: output.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=SLIDES/'sources.json')
    parser.add_argument('--source-root', type=Path, help='default: manifest directory')
    parser.add_argument('--output', type=Path, default=SLIDES)
    parser.add_argument('--faces', type=int, nargs=6, metavar='SLIDE', help='-Z +X +Z -X -Y +Y slide IDs; default: first six manifest entries')
    args = parser.parse_args()
    try:
        entries = load_manifest(args.manifest, args.faces)
        package = bake(entries, args.source_root or args.manifest.parent)
        receipt = {'manifest_sha256': hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
                   'package_sha256': hashlib.sha256(package).hexdigest(),
                   'faces': [e['slide'] for e in entries]}
        args.output.mkdir(parents=True, exist_ok=True)
        atomic_write(args.output/'gallery.cga', package)
        atomic_write(args.output/'gallery.json', (json.dumps(receipt, indent=2)+'\n').encode())
    except (ValueError, OSError) as error:
        parser.error(str(error))
    print(f'six faces: {receipt["faces"]}; {len(package):,} encoded bytes; tiers: {[e["Size"] for e in entries]}')

if __name__ == '__main__': main()
