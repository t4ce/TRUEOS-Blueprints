#[path = "../../crates/cubes-protocol/src/gallery.rs"]
#[allow(dead_code)]
mod gallery;
#[path = "../../crates/cubes-protocol/src/vfx.rs"]
#[allow(dead_code)]
mod vfx;
use std::{env, fs, path::Path, process::Command};

fn catalog(directory: &str, constant: &str, expected: usize) -> String {
    let mut paths = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {directory}: {error}"))
        .map(|entry| entry.expect("catalog entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "cubes")
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), expected, "unexpected {directory} catalog size");

    let mut source = format!("const {constant}: &[Blob] = &[\n");
    for path in paths {
        let bytes = fs::read(&path).expect("read .cubes file");
        assert!(
            bytes.len() >= 16
                && &bytes[..4] == b"CUBE"
                && bytes[4] == 1
                && bytes[7] == 8
                && bytes[11] == 4,
            "{} is not strict-grid CUBES v1",
            path.display()
        );
        let absolute = fs::canonicalize(&path).expect("canonical catalog path");
        let name = path.file_name().unwrap().to_str().unwrap();
        source.push_str(&format!(
            "Blob {{ name: {name:?}, bytes: include_bytes!({absolute:?}) }},\n"
        ));
    }
    source.push_str("];\n");
    source
}

// Packaging copies this app below its target directory. Resolve the shared
// authored world from the enclosing Blueprint workspace, not a staging depth.
fn world1_source() -> std::path::PathBuf {
    if let Some(path) = env::var_os("CUBES_WORLD1_SOURCE") {
        return fs::canonicalize(&path)
            .unwrap_or_else(|error| panic!("CUBES_WORLD1_SOURCE {path:?}: {error}"));
    }
    let manifest = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    // The packager also stages a workspace mirror; use the outer source workspace.
    let workspace = manifest
        .ancestors()
        .filter(|root| {
            root.join("crates/cubes-protocol/Cargo.toml").is_file()
                && root.join("api/Cargo.toml").is_file()
        })
        .last()
        .expect("cannot locate Blueprint workspace; set CUBES_WORLD1_SOURCE");
    let path = workspace
        .parent()
        .unwrap()
        .join("Cubes/Cube/lvl27/world_01_sky.cubes");
    fs::canonicalize(&path).unwrap_or_else(|error| {
        panic!(
            "world1 source {}: {error}; set CUBES_WORLD1_SOURCE if Cubes is elsewhere",
            path.display()
        )
    })
}

fn main() {
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=slides");
    let mut source = catalog("assets", "ASSETS", 49);
    println!("cargo:rerun-if-env-changed=CUBES_WORLD1_SOURCE");
    let world = world1_source();
    println!("cargo:rerun-if-changed={}", world.display());
    source.push_str(&format!(
        "const WORLD1: &[u8] = include_bytes!({world:?});\n"
    ));
    println!("cargo:rerun-if-changed=tools/prepare_slides.py");
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Gallery sources, face selection and VFX lifetimes are authored inputs; bake
    // both embedded packages into Cargo's output directory so ordinary builds
    // never rewrite the catalog.
    let prepared = Command::new("python3")
        .args([
            "-B",
            "tools/prepare_slides.py",
            "--manifest",
            "slides/sources.json",
            "--gallery",
            "slides/gallery.json",
            "--output",
        ])
        .arg(&out)
        .output()
        .expect("gallery preparation requires python3 and Pillow");
    assert!(
        prepared.status.success(),
        "gallery preparation failed (requires Python Pillow):\n{}\n{}",
        String::from_utf8_lossy(&prepared.stdout),
        String::from_utf8_lossy(&prepared.stderr)
    );
    let manifest = fs::read("slides/sources.json").expect("gallery manifest");
    let bytes = fs::read(out.join("gallery.cga")).expect("run tools/prepare_slides.py");
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("gallery.json")).expect("gallery bake receipt"))
            .unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        receipt["manifest_sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(&manifest)),
        "sources.json changed; run tools/prepare_slides.py"
    );
    assert_eq!(
        receipt["package_sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(&bytes)),
        "gallery package changed; run tools/prepare_slides.py"
    );
    assert!(
        gallery::Layout::parse(&bytes).is_some(),
        "invalid gallery package"
    );
    let digest = Sha256::digest(&bytes);
    let revision = u32::from_le_bytes(digest[..4].try_into().unwrap());
    let path = fs::canonicalize(out.join("gallery.cga")).unwrap();
    source.push_str(&format!("const GALLERY: &[u8] = include_bytes!({path:?});\nconst GALLERY_REVISION: u32 = {revision};\n"));
    let bundle_path = fs::canonicalize(out.join("vfx.bin")).unwrap();
    let bundle = fs::read(&bundle_path).unwrap();
    source.push_str(&format!(
        "const VFX_BYTES: &[u8] = include_bytes!({bundle_path:?});\nconst VFX: &[Vfx] = &[\n"
    ));
    for effect in receipt["vfx_catalog"].as_array().expect("VFX catalog") {
        let name = effect["name"].as_str().unwrap();
        let start = effect["offset"].as_u64().unwrap() as usize;
        let end = start + effect["length"].as_u64().unwrap() as usize;
        let bytes = &bundle[start..end];
        assert!(vfx::Sequence::parse(bytes).is_some(), "invalid VFX {name}");
        let digest = Sha256::digest(bytes);
        assert_eq!(effect["sha256"].as_str().unwrap(), format!("{digest:x}"));
        let revision = u32::from_le_bytes(digest[..4].try_into().unwrap());
        source.push_str(&format!(
            "Vfx {{ name: {name:?}, start: {start}, end: {end}, revision: {revision} }},\n"
        ));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("catalog.rs"),
        source,
    )
    .expect("write embedded catalog");
}
