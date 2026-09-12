#!/usr/bin/env python3
"""Check the HTML's actual projection, image upload orientation and fixed opposite-side view."""
import re
import json
from prepare_slides import TIERS
import subprocess
import tempfile
from pathlib import Path

html = (Path(__file__).with_name('CubeImage.html')).read_text()
script = re.search(r'<script>([\s\S]*?)</script>', html).group(1)
with tempfile.TemporaryDirectory(prefix='cube-image-') as folder:
    source = Path(folder)/'preview.js'
    source.write_text(script)
    subprocess.run(['node', '--check', str(source)], check=True)

# Execute the arithmetic from the real GLSL helper, without substituting a
# second mapping implementation. vec2 has the same two-component semantics.
body = re.search(r'vec2 surfaceUV\(vec3 p\)\{([^}]+)\}', script).group(1)
upload = re.findall(r'gl\.pixelStorei\(gl\.UNPACK_FLIP_Y_WEBGL,[^;]+;', script)
assert len(upload) == 1
view = re.search(r'let distance=[^;]+;', script).group(0)
camera_body = re.search(r'function updateCamera\(\)\{([^}]+)\}', script).group(1)
presets=json.loads(re.search(r'const PRESETS=(\{[^;]+\});', script).group(1))
assert presets=={str(side):{'blocks':blocks,'pixels':pixels} for side,blocks,pixels in TIERS.values()}
select=re.search(r'<select id="sourceSize"[^>]*>(.*?)</select>',html).group(1)
assert re.findall(r'<option value="(\d+)"',select)==['48','128','256']
assert 'type="range"' not in html and 'sourceFit' not in script
assert all(name not in script for name in ('voxelP','fakeP','renderBake','surfaceCropScale'))
normalizer=re.search(r'function normalizeSourceImage\(img\)\{([\s\S]*?)\n\}',script).group(1)
assert 'autoRotate' not in script and 'id="autoRotate"' not in html
assert not re.search(r"addEventListener\('pointer(?:down|move|up|cancel)'", script)
assert not re.search(r'\b(?:yaw|pitch)\b', script)
check = '''const assert=require('node:assert/strict');
const vec2=(x,y)=>[x,y];
function surfaceUV(p){BODY}
const gl={UNPACK_FLIP_Y_WEBGL:1,pixelStorei:(_,flip)=>gl.flip=flip};
UPLOAD
// Source corners: red/green on top, blue/white on bottom. The texture upload
// must retain source row order, since surfaceUV already maps +Y to V=0.
const source=['red','green','blue','white'];
function sample(uv){
 const row=Math.min(1,Math.floor(uv[1]*2));
 return source[(gl.flip?1-row:row)*2+Math.min(1,Math.floor(uv[0]*2))];
}
for(const x of [-1,0,1]){
 assert.equal(sample(surfaceUV({x,y:1,z:-1})),'red');
 assert.equal(sample(surfaceUV({x,y:1,z:1})),'green');
 assert.equal(sample(surfaceUV({x,y:-1,z:-1})),'blue');
 assert.equal(sample(surfaceUV({x,y:-1,z:1})),'white');
 assert.deepEqual(surfaceUV({x,y:0,z:0}),[.5,.5]);
}
VIEW;
const camera=[0,0,0],target=[0,0,0];
function updateCamera(){CAMERA}
updateCamera();
assert.deepEqual(camera,[-4.8,0,0]);
distance=2.6; updateCamera();
assert.deepEqual(camera,[-2.6,0,0]);
let sourceSize=48;
let calls=[];
const document={createElement:()=>({getContext:()=>({fillRect:(...args)=>calls.push(['black',...args]),drawImage:(...args)=>calls.push(['image',...args.slice(1)])})})};
function normalizeSourceImage(img){NORMALIZE}
for(const size of [48,128,256]){
 sourceSize=size;
 for(const [w,h] of [[size,size],[size+21,size+10],[size-15,size-9],[size+9,size-7]]){
  calls=[];const canvas=normalizeSourceImage({width:w,height:h});
  assert.equal(canvas.width,size);assert.equal(canvas.height,size);
  const cw=Math.min(size,w),ch=Math.min(size,h);
  assert.deepEqual(calls,[['black',0,0,size,size],['image',Math.max(0,Math.floor((w-size)/2)),Math.max(0,Math.floor((h-size)/2)),cw,ch,Math.floor((size-cw)/2),Math.floor((size-ch)/2),cw,ch]]);
 }
}
console.log('HTML syntax, upright corners, depth-independent UVs and fixed opposite-side view: passed');
'''.replace('BODY', body).replace('UPLOAD', upload[0]).replace('VIEW', view).replace('CAMERA', camera_body).replace('NORMALIZE', normalizer)
subprocess.run(['node', '-e', check], check=True)
