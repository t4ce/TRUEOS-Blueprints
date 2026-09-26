#!/usr/bin/env python3
"""Compare full float4 hardware output (little endian) to RF01 reference."""
import argparse,math,pathlib,struct,sys
p=argparse.ArgumentParser();p.add_argument('reference',type=pathlib.Path);p.add_argument('readback',type=pathlib.Path);p.add_argument('--atol',type=float,default=0.002);p.add_argument('--rtol',type=float,default=0.002);a=p.parse_args()
r=a.reference.read_bytes();g=a.readback.read_bytes()
if len(r)!=len(g) or len(r)%16:sys.exit('FAIL byte length or float4 alignment')
x=struct.unpack('<'+'f'*(len(r)//4),r);y=struct.unpack('<'+'f'*(len(g)//4),g)
errors=[]
for i,(u,v) in enumerate(zip(x,y)):
    if not math.isfinite(v) or abs(u-v)>a.atol+a.rtol*abs(u):errors.append((i,u,v))
if errors: print('FAIL',len(errors),'components; first:',errors[:8]);sys.exit(1)
print('PASS',len(x)//4,'float4 records; numeric comparison, not visual proof')
