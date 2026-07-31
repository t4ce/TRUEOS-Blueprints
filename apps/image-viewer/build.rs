use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = [
        manifest_dir.join("../../../TRUEOS/logo.jpg"),
        manifest_dir.join("../../TRUEOS/logo.jpg"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
    .expect("TRUEOS/logo.jpg is required for the built-in image-viewer source");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("builtin-logo.jpg");
    fs::copy(&source, output).expect("failed to stage TRUEOS/logo.jpg");
    println!("cargo:rerun-if-changed={}", source.display());
}
