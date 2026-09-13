#!/usr/bin/env python3
"""Bake six tiered CubeImage slabs into one PNG atlas (Pillow, offline only)."""
import argparse
import hashlib
import io
import json
import math
import os
import re
import tempfile
from pathlib import Path
from PIL import Image, ImageOps

SLIDES = Path(__file__).resolve().parents[1] / 'slides'
# source resolution, 1 x N x N assembly, projected texture pixels per slab face.
TIERS = {'tier1': (48, 6, 6), 'tier2': (128, 8, 16), 'tier3': (256, 16, 128)}
EXTENSIONS = ('.png', '.jpg', '.jpeg', '.jgp')
VFX_SIDE = 32
VFX_PERIOD_MS = 300


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
        if type(entry['slide']) is not int or entry['slide'] < 0 or entry['slide'] in ids:
            raise ValueError('slide IDs must be unique non-negative integers')
        ids.add(entry['slide'])
        if not isinstance(entry['Size'], str) or entry['Size'] not in TIERS or not isinstance(entry['source'], str):
            raise ValueError('Size must be tier1, tier2 or tier3')
        if Path(entry['source']).suffix.lower() not in EXTENSIONS:
            raise ValueError('source must be a PNG/JPEG filename')
    by_id = {entry['slide']: entry for entry in entries}
    selected = faces if faces is not None else [entry['slide'] for entry in entries[:6]]
    if not isinstance(selected, list) or len(selected) != 6 or any(type(i) is not int for i in selected):
        raise ValueError('faces must contain six integer slide IDs')
    if len(set(selected)) != 6 or any(i not in by_id for i in selected):
        raise ValueError('select six different existing slide IDs')
    return [by_id[i] for i in selected]


def load_faces(path):
    """Gallery faces are editable; digest fields are generated bookkeeping."""
    if not path.exists():
        return None
    gallery = json.loads(path.read_text())
    if not isinstance(gallery, dict) or 'faces' not in gallery:
        raise ValueError('gallery.json requires a faces array')
    faces = gallery['faces']
    if not isinstance(faces, list) or len(faces) != 6 or any(type(i) is not int or i < 0 for i in faces):
        raise ValueError('faces must contain six non-negative integer slide IDs')
    if len(set(faces)) != 6:
        raise ValueError('faces must select six different slide IDs')
    return faces


def load_frames(directory):
    """Read numbered 32px PNG frames and return sparse indexed pixels."""
    def frame_number(path):
        match = re.search(r'(\d+)$', path.stem)
        if not match:
            raise ValueError(f'VFX frame needs a trailing number: {path.name}')
        return int(match.group(1))
    paths = sorted(directory.glob('*.png'), key=frame_number)
    if not paths or len(paths) > 255:
        raise ValueError('VFX requires 1..255 PNG frames')
    numbers = [frame_number(path) for path in paths]
    if len(set(numbers)) != len(numbers):
        raise ValueError('VFX frame numbers must be unique')
    rgba_frames = []
    colors = set()
    for path in paths:
        with Image.open(path) as source:
            image = ImageOps.exif_transpose(source).convert('RGBA')
            if image.size != (VFX_SIDE, VFX_SIDE):
                raise ValueError(f'{path.name} must be {VFX_SIDE}x{VFX_SIDE}')
            rgba = list(image.get_flattened_data())
        rgba_frames.append(rgba)
        colors.update((r,g,b) for r,g,b,a in rgba if a != 0)
    palette = sorted(colors)
    if not palette or len(palette) > 255:
        raise ValueError('VFX requires 1..255 visible RGB colors')
    indices = {color: index for index, color in enumerate(palette)}
    frames = []
    for rgba in rgba_frames:
        frames.append([(x, y, indices[(r,g,b)])
                       for y in range(VFX_SIDE) for x in range(VFX_SIDE)
                       for r,g,b,a in [rgba[y*VFX_SIDE+x]] if a != 0])
    return paths, palette, frames


def bake_vfx(palette, frames):
    """One record per unchanged pixel lifetime, with exclusive end-frame removal."""
    grid = [dict(((x, y), color) for x, y, color in frame) for frame in frames]
    runs = bytearray()
    for y in range(VFX_SIDE):
        for x in range(VFX_SIDE):
            start = 0
            while start < len(frames):
                color = grid[start].get((x, y))
                end = start + 1
                while end < len(frames) and grid[end].get((x, y)) == color:
                    end += 1
                if color is not None:
                    runs.extend((x, y, color, start, end))
                start = end
    period = VFX_PERIOD_MS
    return (b'VFX1' + bytes([1, VFX_SIDE, VFX_SIDE, len(frames)])
            + period.to_bytes(2, 'little') + bytes([len(palette), 0])
            + bytes(v for color in palette for v in color) + runs)


def load_vfx_catalog(root):
    """Fixed Pixel VFX pack, sharing exactly the client's RGB555 display colours."""
    def display_rgb(rgb):
        return tuple((((v * 31 + 127) // 255) * 255 + 15) // 31 for v in rgb)
    sources = []
    colors = set()
    for directory in sorted(root.glob('*/*')):
        if not directory.is_dir():
            continue
        _, palette, frames = load_frames(directory)
        palette = [display_rgb(rgb) for rgb in palette]
        colors.update(palette)
        sources.append((directory.relative_to(root).as_posix(), palette, frames))
    if not sources or len(colors) > 255:
        raise ValueError('VFX catalog requires effects and at most 255 RGB555 colours')
    palette = sorted(colors)
    indices = {rgb: i for i, rgb in enumerate(palette)}
    return palette, [(name, [[(x, y, indices[local[index]]) for x, y, index in frame]
                             for frame in frames]) for name, local, frames in sources]


def bake(entries, source_root, vfx_palette=()):
    tile = max(TIERS[e['Size']][2] for e in entries)+2
    # The final white row supplies one constant texel for the client's central
    # c4 landmark while preserving one atlas and one retained draw.
    atlas = Image.new('RGB', (tile*3, tile*2+1))
    for x in range(atlas.width):
        atlas.putpixel((x, atlas.height-1), (255, 255, 255))
    if len(vfx_palette) + 1 > atlas.width:
        raise ValueError('VFX palette exceeds the gallery atlas row')
    for index, color in enumerate(vfx_palette):
        atlas.putpixel((index + 1, atlas.height - 1), color)
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
    package = b'CGA1' + bytes([8, 6] + [int(e['Size'][-1]) for e in entries]) + bytes([1, 1, 0, 4]) + png.getvalue()
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
    parser.add_argument('--gallery', type=Path, help='face selection; default: gallery.json beside the manifest')
    parser.add_argument('--source-root', type=Path, help='default: manifest directory')
    parser.add_argument('--output', type=Path, default=SLIDES)
    parser.add_argument('--vfx-root', type=Path, help='pack frame root; default: pixvfx/Frames beside manifest')
    parser.add_argument('--faces', type=int, nargs=6, metavar='SLIDE', help='-Z +X +Z -X -Y +Y slide IDs; default: gallery.json faces, or first six manifest entries for a new gallery')
    args = parser.parse_args()
    try:
        faces = args.faces if args.faces is not None else load_faces(args.gallery or args.manifest.parent/'gallery.json')
        entries = load_manifest(args.manifest, faces)
        pack_root = args.vfx_root or (args.source_root or args.manifest.parent) / 'pixvfx/Frames'
        vfx_palette, catalog = load_vfx_catalog(pack_root)
        package = bake(entries, args.source_root or args.manifest.parent, vfx_palette)
        bundle = bytearray()
        catalog_receipt = []
        for name, frames in catalog:
            encoded = bake_vfx(vfx_palette, frames)
            catalog_receipt.append({'name': name, 'offset': len(bundle), 'length': len(encoded),
                                    'sha256': hashlib.sha256(encoded).hexdigest(),
                                    'frames': len(frames), 'runs': (len(encoded)-12-3*len(vfx_palette))//5,
                                    'full_frame_bytes': sum(map(len, frames))*3})
            bundle.extend(encoded)
        receipt = {'manifest_sha256': hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
                   'package_sha256': hashlib.sha256(package).hexdigest(),
                   'vfx_catalog': catalog_receipt,
                   'faces': [e['slide'] for e in entries]}
        args.output.mkdir(parents=True, exist_ok=True)
        atomic_write(args.output/'gallery.cga', package)
        atomic_write(args.output/'vfx.bin', bundle)
        atomic_write(args.output/'gallery.json', (json.dumps(receipt, indent=2)+'\n').encode())
    except (ValueError, OSError) as error:
        parser.error(str(error))
    print(f'six faces: {receipt["faces"]}; {len(package):,} gallery bytes; '
          f'VFX: {len(catalog)} effects / {len(bundle):,} lifetime-encoded bytes')

if __name__ == '__main__': main()
