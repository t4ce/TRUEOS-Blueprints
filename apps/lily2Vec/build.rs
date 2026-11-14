use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=LILY2VEC_VNNI_SOURCE");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("vnni_module.rs");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        fs::write(output, "").unwrap();
        return;
    }
    // The SDK stages root packages deeper in the Blueprint repository. Locate
    // the authored kernel file from either layout; never duplicate its source.
    let source = if let Some(path) = env::var_os("LILY2VEC_VNNI_SOURCE") {
        PathBuf::from(path)
    } else {
        manifest
            .ancestors()
            .flat_map(|root| {
                [
                    root.join("src/turbo/vnni.rs"),
                    root.join("TRUEOS/src/turbo/vnni.rs"),
                ]
            })
            .find(|path| path.is_file())
            .expect(
                "shared VNNI helper missing; set LILY2VEC_VNNI_SOURCE to TRUEOS/src/turbo/vnni.rs",
            )
    };
    let source = source.canonicalize().expect("resolve shared VNNI source");
    println!("cargo:rerun-if-changed={}", source.display());
    fs::write(
        output,
        format!("#[path = {:?}]\npub mod vnni;\n", source.to_str().unwrap()),
    )
    .unwrap();
}
