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

fn main() {
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=slides");
    let mut source = catalog("assets", "ASSETS", 49);
    println!("cargo:rerun-if-changed=worlds");
    let world = fs::canonicalize("worlds/demo.cubes").expect("demo world");
    source.push_str(&format!("const DEMO_WORLD: &[u8] = include_bytes!({world:?});\n"));
    source.push_str(&world_catalog());
    println!("cargo:rerun-if-changed=tools/prepare_slides.py");
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Gallery sources, face selection and VFX lifetimes are authored inputs; bake
    // both embedded packages into Cargo's output directory so ordinary builds
    // never rewrite the catalog.
    let prepared = Command::new("python3")
        .args(["-B", "tools/prepare_slides.py", "--manifest", "slides/sources.json",
               "--gallery", "slides/gallery.json", "--output"])
        .arg(&out).output().expect("gallery preparation requires python3 and Pillow");
    assert!(prepared.status.success(), "gallery preparation failed (requires Python Pillow):\n{}\n{}",
        String::from_utf8_lossy(&prepared.stdout), String::from_utf8_lossy(&prepared.stderr));
    let manifest = fs::read("slides/sources.json").expect("gallery manifest");
    let bytes = fs::read(out.join("gallery.cga")).expect("run tools/prepare_slides.py");
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(out.join("gallery.json")).expect("gallery bake receipt")).unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(receipt["manifest_sha256"].as_str().unwrap(), format!("{:x}", Sha256::digest(&manifest)),
        "sources.json changed; run tools/prepare_slides.py");
    assert_eq!(receipt["package_sha256"].as_str().unwrap(), format!("{:x}", Sha256::digest(&bytes)),
        "gallery package changed; run tools/prepare_slides.py");
    assert!(gallery::Layout::parse(&bytes).is_some(), "invalid gallery package");
    let digest = Sha256::digest(&bytes);
    let revision = u32::from_le_bytes(digest[..4].try_into().unwrap());
    let path = fs::canonicalize(out.join("gallery.cga")).unwrap();
    source.push_str(&format!("const GALLERY: &[u8] = include_bytes!({path:?});\nconst GALLERY_REVISION: u32 = {revision};\n"));
    let bundle_path = fs::canonicalize(out.join("vfx.bin")).unwrap();
    let bundle = fs::read(&bundle_path).unwrap();
    source.push_str(&format!("const VFX_BYTES: &[u8] = include_bytes!({bundle_path:?});\nconst VFX: &[Vfx] = &[\n"));
    for effect in receipt["vfx_catalog"].as_array().expect("VFX catalog") {
        let name = effect["name"].as_str().unwrap();
        let start = effect["offset"].as_u64().unwrap() as usize;
        let end = start + effect["length"].as_u64().unwrap() as usize;
        let bytes = &bundle[start..end];
        assert!(vfx::Sequence::parse(bytes).is_some(), "invalid VFX {name}");
        let digest = Sha256::digest(bytes);
        assert_eq!(effect["sha256"].as_str().unwrap(), format!("{digest:x}"));
        let revision = u32::from_le_bytes(digest[..4].try_into().unwrap());
        source.push_str(&format!("Vfx {{ name: {name:?}, start: {start}, end: {end}, revision: {revision} }},\n"));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("catalog.rs"),
        source,
    )
    .expect("write embedded catalog");
}

fn world_catalog() -> String {
    use sha2::{Digest, Sha256};
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read("worlds/lvl27/platform-hulls.json").unwrap()).unwrap();
    assert_eq!(manifest["version"], 1);
    let worlds = manifest["worlds"].as_array().unwrap();
    assert_eq!(worlds.len(), 27);
    let mut source = String::from("const WORLDS: &[&[u8]] = &[\n");
    for (index, metadata) in worlds.iter().enumerate() {
        let name = metadata["filename"].as_str().unwrap();
        assert!(name.starts_with(&format!("world_{:02}_", index+1)) && !name.contains('/'));
        let bytes = fs::read(Path::new("worlds/lvl27").join(name)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), metadata["sha256"].as_str().unwrap(), "stale platform metadata");
        let body = serde_json::to_vec(&serde_json::json!({"id":index+1,"cubes":bytes,"platforms":metadata})).unwrap();
        assert!(body.len() <= 4*1024*1024);
        let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(format!("world-{}.json",index+1));
        fs::write(&path,body).unwrap();
        source.push_str(&format!("include_bytes!({path:?}),\n"));
    }
    source.push_str("];\n"); source
}
