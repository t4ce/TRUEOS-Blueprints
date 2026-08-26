use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let asset_root = manifest_dir
        .join("..")
        .join("monaco")
        .join("static")
        .join("monaco")
        .join("vs");
    println!("cargo:rerun-if-changed={}", asset_root.display());

    if !asset_root.is_dir() {
        panic!(
            "missing shared Monaco assets at {}; build apps/monaco assets first",
            asset_root.display()
        );
    }

    let mut files = Vec::new();
    collect_files(&asset_root, &asset_root, &mut files);
    files.sort();

    let mut generated = String::from(
        "pub struct StaticAsset {\n    pub path: &'static str,\n    pub mime: &'static str,\n    pub bytes: &'static [u8],\n}\n\npub static STATIC_ASSETS: &[StaticAsset] = &[\n",
    );
    for rel in files {
        let rel_slash = rel.to_string_lossy().replace('\\', "/");
        let mime = mime_for(&rel_slash);
        generated.push_str(&format!(
            "    StaticAsset {{ path: {:?}, mime: {:?}, bytes: include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../monaco/static/monaco/vs/{}\")) }},\n",
            rel_slash, mime, rel_slash
        ));
    }
    generated.push_str("];\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("strudel_monaco_assets.rs"), generated).unwrap();
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if path.is_file() {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}
