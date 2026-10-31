use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=TRUEOS_API_SOURCE_ROOT");
    println!("cargo:rerun-if-changed=api-prm");
    if env::var_os("CARGO_FEATURE_API_REFERENCE").is_none() {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = env::var_os("TRUEOS_API_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../../TRUEOS"));
    let generator = source.join("tools/build_api_prm.py");
    if generator.is_file() {
        assert!(
            Command::new("python3")
                .arg(generator)
                .arg("--output")
                .arg(root.join("api-prm"))
                .status()
                .expect("run TRUEOS API reference generator")
                .success(),
            "generate IDE API reference"
        );
    }
    let verify = r#"
import hashlib, json, pathlib, sys
root, source = map(pathlib.Path, sys.argv[1:])
manifest = json.loads((root / 'manifest.json').read_text())
for name, info in manifest['files'].items():
    assert pathlib.Path(name).name == name, 'invalid API reference path'
    data = (root / name).read_bytes()
    assert len(data) == info['bytes'] and hashlib.sha256(data).hexdigest() == info['sha256'], 'API reference hash mismatch: ' + name
for name in manifest['source']['files']:
    if (source / name).is_file():
        print('cargo:rerun-if-changed=' + str(source / name))
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(verify)
        .arg(root.join("api-prm"))
        .arg(source)
        .output()
        .expect("verify IDE API reference");
    assert!(
        output.status.success(),
        "invalid IDE API reference: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
