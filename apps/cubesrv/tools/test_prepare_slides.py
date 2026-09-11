#!/usr/bin/env python3
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from prepare_slides import prepare, normalize, slide_paths

class PreparationTests(unittest.TestCase):
    def test_small_image_is_padded_without_upscaling(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'small.png'
            image = Image.new('RGB', (10, 20), (40, 80, 120))
            image.putpixel((0, 0), (240, 80, 120))
            image.save(path)
            output = prepare(path)
            self.assertEqual(output.size, (512, 512))
            self.assertEqual(output.getpixel((0, 0)), (41, 80, 120))
            self.assertEqual(output.getpixel((251, 246)), (240, 80, 120))
    def test_large_landscape_is_center_cropped(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'wide.png'
            image = Image.new('RGB', (1024, 512), (255, 0, 0))
            image.paste((0, 255, 0), (256, 0, 768, 512))
            image.save(path)
            self.assertEqual(prepare(path).getextrema(), ((0, 0), (255, 255), (0, 0)))
    def test_checked_in_slides_are_standard_images(self):
        slides = Path(__file__).resolve().parents[1] / 'slides'
        self.assertGreater(len(slide_paths(slides)), 0)
        self.assertEqual(list(slides.glob('*.rgb')), [])
        for path in slide_paths(slides):
            with Image.open(path) as image:
                self.assertEqual(image.size, (512, 512))
                self.assertEqual(image.mode, 'RGB')
                self.assertIn(image.format, ('PNG', 'JPEG'))
                self.assertEqual(image.getexif().get(274, 1), 1)
                self.assertLessEqual(path.stat().st_size, 4*1024*1024)
                image.load()

    def test_all_filename_variants_normalize_and_are_idempotent(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for suffix in ('.png', '.PNG', '.jpg', '.jpeg', '.jgp', '.JGP'):
                path = root / f'image{suffix}'
                fmt = 'PNG' if suffix.lower() == '.png' else 'JPEG'
                Image.new('RGB', (396, 697), (60, 90, 120)).save(path, format=fmt)
                self.assertTrue(normalize(path))
                with Image.open(path) as result:
                    self.assertEqual(result.size, (512, 512))
                    self.assertEqual(result.format, fmt)
                    self.assertEqual(result.mode, 'RGB')
                before = path.read_bytes()
                self.assertFalse(normalize(path))
                self.assertEqual(path.read_bytes(), before)
            (root/'not-an-image.jpg').mkdir()
            self.assertEqual(len(slide_paths(root)), 6)

    def test_exif_orientation_is_applied_before_padding(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'rotated.jpg'
            exif = Image.Exif()
            exif[274] = 6
            image = Image.new('RGB', (800, 400), (255, 0, 0))
            image.paste((0, 0, 255), (400, 0, 800, 400))
            image.save(path, exif=exif)
            self.assertTrue(normalize(path))
            with Image.open(path) as result:
                self.assertEqual(result.size, (512, 512))
                self.assertEqual(result.getexif().get(274, 1), 1)
                self.assertGreater(result.getpixel((256, 100))[0], 240)
                self.assertGreater(result.getpixel((256, 400))[2], 240)

    def test_fully_transparent_png_has_a_defined_background(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'transparent.png'
            Image.new('RGBA', (30, 20), (255, 0, 0, 0)).save(path)
            self.assertTrue(normalize(path))
            with Image.open(path) as result:
                self.assertEqual(result.mode, 'RGB')
                self.assertEqual(result.getextrema(), ((0,0), (0,0), (0,0)))

if __name__ == '__main__': unittest.main()
