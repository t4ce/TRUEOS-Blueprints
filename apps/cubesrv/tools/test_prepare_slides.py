#!/usr/bin/env python3
"""Exercise the tier importer, source contract and six-face encoded atlas."""
import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from prepare_slides import SLIDES, TIERS, prepare, texture, bake, load_manifest, aligned_grid

class PreparationTests(unittest.TestCase):
    def test_exact_tiers_crop_nearest_and_alpha(self):
        self.assertEqual(list(TIERS.values()), [(64,8,8),(128,10,21),(256,14,31),(512,16,320)])
        self.assertEqual([aligned_grid(tier) for tier in TIERS], [8,20,28,320])
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'source.png'
            image = Image.new('RGBA',(200,100),(255,0,0,255))
            image.paste((0,255,0,255),(50,0,150,100))
            image.save(path)
            for tier,(side,_,pixels) in TIERS.items():
                result = prepare(path,tier)
                self.assertEqual(result.size,(side,side))
                self.assertEqual(result.getextrema(),((0,0),(255,255),(0,0)))
                self.assertEqual(texture(result,tier).size,(aligned_grid(tier),)*2)
            Image.new('RGBA',(10,20),(255,0,0,0)).save(path)
            self.assertEqual(prepare(path,'tier1').getextrema(),((0,0),)*3)

    def test_center_sampling_matches_html_at_every_grid_cell(self):
        for tier,(side,_,pixels) in TIERS.items():
            grid = aligned_grid(tier)
            image=Image.new('RGB',(side,side))
            image.putdata([(x%256,y%256,(x+y)%256) for y in range(side) for x in range(side)])
            result=texture(image,tier)
            for y in range(grid):
                for x in range(grid):
                    import math
                    crop=grid/pixels
                    rgb=image.getpixel((int((.5+((x+.5)/grid-.5)*crop)*side),int((.5+((y+.5)/grid-.5)*crop)*side)))
                    self.assertEqual(result.getpixel((x,y)),tuple(math.floor(min(1,v/255*1.05)*15+.5)*17 for v in rgb))

    def test_manifest_rejects_unknown_sizes_dimensions_duplicates_and_face_ids(self):
        entries=[{'slide':i+1,'source':f'{i}.png','Size':'tier1'} for i in range(6)]
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'sources.json'
            path.write_text(json.dumps(entries))
            self.assertEqual(load_manifest(path),entries)
            self.assertEqual(load_manifest(path,[6,5,4,3,2,1]),list(reversed(entries)))
            for face_ids in ([1]*6,[1,2,3,4,5,99]):
                with self.assertRaises(ValueError): load_manifest(path,face_ids)
            for key,value in [('Size','tier5'),('width',512),('slide',2)]:
                bad=[dict(e) for e in entries];bad[0][key]=value
                path.write_text(json.dumps(bad))
                with self.assertRaises(ValueError): load_manifest(path)

    def test_mixed_tier_atlas_order_padding_and_package(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            entries=[]
            colors=[(255,0,0),(0,255,0),(0,0,255),(255,255,0),(0,255,255),(255,0,255)]
            for i,(tier,color) in enumerate(zip(['tier1','tier2','tier3','tier4','tier3','tier4'],colors)):
                Image.new('RGB',(TIERS[tier][0],)*2,color).save(root/f'{i}.png')
                entries.append({'slide':i+1,'source':f'{i}.png','Size':tier})
            data=bake(entries,root)
            self.assertEqual(data[:16],b'CGA1'+bytes([2,6,1,2,3,4,3,4])+bytes(4))
            atlas=Image.open(io.BytesIO(data[16:]));self.assertEqual(atlas.size,(966,644))
            for i,(entry,color) in enumerate(zip(entries,colors)):
                n=aligned_grid(entry['Size']);x,y=i%3*322,i//3*322
                for dx,dy in ((0,0),(1,1),(n,n),(n+1,n+1)):
                    self.assertEqual(atlas.getpixel((x+dx,y+dy)),color)
            self.assertEqual(data,bake(entries,root))

    def test_checked_in_package_matches_manifest_receipt_and_sources(self):
        manifest=SLIDES/'sources.json';receipt=json.loads((SLIDES/'gallery.json').read_text())
        data=(SLIDES/'gallery.cga').read_bytes()
        self.assertEqual(hashlib.sha256(manifest.read_bytes()).hexdigest(),receipt['manifest_sha256'])
        self.assertEqual(hashlib.sha256(data).hexdigest(),receipt['package_sha256'])
        self.assertEqual(data,bake(load_manifest(manifest,receipt['faces']),SLIDES))
        self.assertLessEqual(len(data),4*1024*1024)

if __name__ == '__main__': unittest.main()
