use std::{env, fs, path::PathBuf};

fn main() {
    let assets = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    for shader in [
        "mandelbrot",
        "cube_field",
        "nguyen",
        "palette_grid",
        "cosmic_strands",
    ] {
        let mut parts = Vec::new();
        for file in [
            "kernel.bin",
            "kernel.spv",
            "input.glsl",
            "kernel.clcpp",
            "kernel.manifest.json",
            "kernel.contract.rs",
        ] {
            let path = assets.join(shader).join(file);
            println!("cargo:rerun-if-changed={}", path.display());
            parts.push(fs::read(path).expect("read ShaderToy package component"));
        }
        let mut expected = b"STPKG01\0".to_vec();
        for part in &parts {
            expected.extend_from_slice(&u32::try_from(part.len()).unwrap().to_le_bytes());
        }
        for part in parts {
            expected.extend_from_slice(&part);
        }
        let package = assets.join(format!("{shader}.stpkg"));
        println!("cargo:rerun-if-changed={}", package.display());
        if fs::read(package).expect("read ShaderToy package") != expected {
            panic!(
                "{shader}: stale package; run TRUEOS/tools/shadertoy-cpp-offline/package_blueprint.py and review any trust changes"
            );
        }
    }
}
