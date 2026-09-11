#!/usr/bin/env python3
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from prepare_slides import prepare

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
        self.assertGreater(len(list(slides.glob('*.png'))), 0)
        self.assertEqual(list(slides.glob('*.rgb')), [])
        for path in slides.glob('*.png'):
            with Image.open(path) as image:
                self.assertEqual(image.size, (512, 512))
                self.assertEqual(image.mode, 'RGB')
                image.load()

if __name__ == '__main__': unittest.main()
