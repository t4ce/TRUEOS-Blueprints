use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let texture = manifest_dir
        .ancestors()
        .map(|dir| dir.join("bfly/shaders/trueos/bfly-sun.rgba8"))
        .find(|path| path.is_file())
        .expect("Bfly Sun texture in the Blueprint checkout");
    println!("cargo:rerun-if-changed={}", texture.display());
    println!("cargo:rustc-env=DEMO_SUN_TEXTURE={}", texture.display());
}
