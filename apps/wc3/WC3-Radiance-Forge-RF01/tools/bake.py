#!/usr/bin/env python3
"""Drive TRUEOS's existing locked bakery, without publishing or weakening its gates."""
import argparse,pathlib,subprocess,sys,shlex
ROOT=pathlib.Path(__file__).resolve().parents[1]
KERNELS=('rf_build_light_lists','rf_direct','rf_indirect','rf_water')
def main():
    p=argparse.ArgumentParser();p.add_argument('--trueos-root',type=pathlib.Path,required=True);p.add_argument('--dry-run',action='store_true');a=p.parse_args()
    repo=a.trueos_root.resolve();bakery=repo/'tools/intel-gpu-bakery/bake.py'
    lock=repo/'tools/intel-gpu-bakery/toolchains/adls-cpp-proof.lock.json'
    profile=repo/'tools/intel-gpu-bakery/profiles/adls-4680-r0c-cpp.json'
    if not all(f.is_file() for f in (bakery,lock,profile)):raise RuntimeError('expected pinned TRUEOS bakery/profile/lock not found')
    for k in KERNELS:
        source=ROOT/'kernels'/f'{k}.clcpp'
        if not source.is_file():raise RuntimeError(f'missing RF01 kernel source: {source}')
        # The upstream bakery already records external source paths and runs
        # Clang from source.parent for reproducibility. Keeping RF01 in its
        # supplied Blueprint package avoids a second, diverging source copy;
        # only no-publish candidate outputs are written under TRUEOS/bld.
        cmd=[sys.executable,str(bakery),'--source',str(source),'--artifact-name',k,'--profile',str(profile),
             '--expect-kernel',k,'--rust-symbol',k+'='+k.upper()+'_ADLS_CPP_ABI_CONTRACT',
             '--toolchain-lock',str(lock),'--repro-check','--build-root',str(repo/'bld/wc3-radiance-forge'/k)]
        print(shlex.join(cmd),flush=True)
        if not a.dry_run:subprocess.run(cmd,cwd=repo,check=True)
    print('Candidate bake only. No publication, runtime admission, or hardware proof is implied.')
if __name__=='__main__':
    try:main()
    except Exception as e:print('ERROR:',e,file=sys.stderr);sys.exit(1)
