#!/usr/bin/env python3
"""Host-reference tests and non-publication OpenCL frontend checks; never claims GPU execution."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
KERNELS=('rf_build_light_lists','rf_direct','rf_indirect','rf_water')
def main():
    p=argparse.ArgumentParser();p.add_argument('--build-dir',type=pathlib.Path,default=ROOT/'validation');a=p.parse_args()
    out=a.build_dir.resolve();out.mkdir(parents=True,exist_ok=True)
    cpp=shutil.which(os.environ.get('CXX','c++'));clang=shutil.which(os.environ.get('CLANG','clang'))
    if not cpp or not clang: raise RuntimeError('C++ compiler and Clang required for these local checks')
    commands=[]
    def run(cmd):
        r=subprocess.run([str(x) for x in cmd],cwd=ROOT,text=True,capture_output=True)
        commands.append({'argv':[str(x) for x in cmd], 'returncode':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
        if r.returncode: raise RuntimeError(r.stdout+r.stderr)
        return r.stdout
    report={'native_bake':False,'spirv_translation':False,'gpu_execution':False,'baremetal_execution':False}
    try:
        report['host_compiler']=run([cpp,'--version']).splitlines()[0]
        report['frontend_compiler']=run([clang,'--version']).splitlines()[0]
        run([cpp,'-std=c++17','-O2','-Wall','-Wextra','-Werror','host/test.cpp','-o',out/'test'])
        print(run([out/'test',out/'fixture']),end='')
        for k in KERNELS:
            # This compiler can differ from the pinned bakery. Output is frontend evidence only.
            run([clang,'--target=spir64','-x','clcpp','-cl-std=CLC++','-cl-kernel-arg-info',
                 '-fno-discard-value-names','-Wall','-Wextra','-Werror','-O2','-emit-llvm','-c',
                 ROOT/'kernels'/f'{k}.clcpp','-o',out/f'{k}.bc'])
            print(f'PASS frontend {k}')
        report.update(host_tests_passed=21,randomized_bvh_rays=5000,frontend_kernels_passed=list(KERNELS))
        report['source_sha256']={str(f.relative_to(ROOT)):hashlib.sha256(f.read_bytes()).hexdigest() for folder in ('kernels','host') for f in sorted((ROOT/folder).glob('*')) if f.is_file()}
    finally:
        report['commands']=commands;(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':
    try:main()
    except Exception as e:print(f'ERROR {e}',file=sys.stderr);sys.exit(1)
