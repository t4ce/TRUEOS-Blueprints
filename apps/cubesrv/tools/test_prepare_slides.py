#!/usr/bin/env python3
"""Three-tier contract, lossless crop/pad placement, and encoded gallery checks."""
import hashlib
import io
import json
import math
import tempfile
import unittest
from pathlib import Path
from PIL import Image
from prepare_slides import SLIDES, TIERS, prepare, texture, bake, load_manifest

class PreparationTests(unittest.TestCase):
    def test_exact_tiers_and_crop_pad_without_scaling(self):
        self.assertEqual(list(TIERS.values()),[(48,6,6),(128,8,16),(256,16,32)])
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'source.png'
            for tier,(side,_,_) in TIERS.items():
                for w,h in [(side,side),(side+21,side+10),(side-15,side-9),(side+9,side-7)]:
                    original=Image.new('RGB',(w,h))
                    original.putdata([(x%255+1,y%255+1,73) for y in range(h) for x in range(w)])
                    original.save(path);result=prepare(path,tier)
                    self.assertEqual(result.size,(side,side))
                    # Compare every destination pixel to a centered, unscaled
                    # source pixel, including odd-size offsets and black edges.
                    left,top=(side-min(w,side))//2,(side-min(h,side))//2
                    sx,sy=max(0,(w-side)//2),max(0,(h-side)//2)
                    for y in range(side):
                        for x in range(side):
                            inside=left<=x<left+min(w,side) and top<=y<top+min(h,side)
                            expected=original.getpixel((x-left+sx,y-top+sy)) if inside else (0,0,0)
                            self.assertEqual(result.getpixel((x,y)),expected)
            Image.new('RGBA',(9,7),(255,0,0,0)).save(path)
            self.assertEqual(prepare(path,'tier1').getextrema(),((0,0),)*3)

    def test_center_sampling_without_an_additional_crop(self):
        for tier,(side,_,pixels) in TIERS.items():
            image=Image.new('RGB',(side,side))
            image.putdata([(x%256,y%256,(x+y)%256) for y in range(side) for x in range(side)])
            result=texture(image,tier)
            self.assertEqual(result.size,(pixels,pixels))
            for y in range(pixels):
                for x in range(pixels):
                    rgb=image.getpixel((int((x+.5)*side/pixels),int((y+.5)*side/pixels)))
                    self.assertEqual(result.getpixel((x,y)),tuple(math.floor(min(1,v/255*1.05)*15+.5)*17 for v in rgb))

    def test_manifest_rejects_removed_tier_and_unbounded_settings(self):
        entries=[{'slide':i+1,'source':f'{i}.png','Size':'tier1'} for i in range(6)]
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'sources.json';path.write_text(json.dumps(entries))
            self.assertEqual(load_manifest(path),entries)
            self.assertEqual(load_manifest(path,[6,5,4,3,2,1]),list(reversed(entries)))
            for ids in ([1]*6,[1,2,3,4,5,99]):
                with self.assertRaises(ValueError):load_manifest(path,ids)
            for key,value in [('Size','tier4'),('Size','tier5'),('width',512),('slide',2)]:
                bad=[dict(e) for e in entries];bad[0][key]=value;path.write_text(json.dumps(bad))
                with self.assertRaises(ValueError):load_manifest(path)

    def test_mixed_atlas_order_border_and_version(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);entries=[]
            colors=[(255,0,0),(0,255,0),(0,0,255),(255,255,0),(0,255,255),(255,0,255)]
            for i,(tier,color) in enumerate(zip(['tier1','tier2','tier3']*2,colors)):
                Image.new('RGB',(TIERS[tier][0],)*2,color).save(root/f'{i}.png')
                entries.append({'slide':i+1,'source':f'{i}.png','Size':tier})
            data=bake(entries,root)
            self.assertEqual(data[:16],b'CGA1'+bytes([4,6,1,2,3,1,2,3])+bytes(4))
            atlas=Image.open(io.BytesIO(data[16:]));self.assertEqual(atlas.size,(102,68))
            for i,(entry,color) in enumerate(zip(entries,colors)):
                n=TIERS[entry['Size']][2];x,y=i%3*34,i//3*34
                for dx,dy in ((0,0),(1,1),(n,n),(n+1,n+1)):
                    self.assertEqual(atlas.getpixel((x+dx,y+dy)),color)
            self.assertEqual(data,bake(entries,root))

    def test_package_matches_manifest_and_current_sources(self):
        manifest=SLIDES/'sources.json';receipt=json.loads((SLIDES/'gallery.json').read_text())
        data=(SLIDES/'gallery.cga').read_bytes()
        self.assertEqual(hashlib.sha256(manifest.read_bytes()).hexdigest(),receipt['manifest_sha256'])
        self.assertEqual(hashlib.sha256(data).hexdigest(),receipt['package_sha256'])
        self.assertEqual(data,bake(load_manifest(manifest,receipt['faces']),SLIDES))
        self.assertLessEqual(len(data),4*1024*1024)

if __name__=='__main__':unittest.main()
