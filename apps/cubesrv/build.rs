use std::{env, fs, path::Path};

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
    source.push_str("const SLIDES: &[Blob] = &[\n");
    let mut slides = fs::read_dir("slides")
        .expect("slides directory")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|v| v.to_str())
                    .is_some_and(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "jgp"
                        )
                    })
        })
        .collect::<Vec<_>>();
    slides.sort();
    assert!(!slides.is_empty(), "expected at least one PNG/JPEG slide");
    for path in slides {
        let path = fs::canonicalize(path).expect("prepared slide");
        let size = fs::metadata(&path).unwrap().len();
        assert!(size > 0 && size <= 4 * 1024 * 1024, "slide transfer size");
        let name = path.file_name().unwrap().to_str().unwrap();
        source.push_str(&format!(
            "Blob {{ name: {name:?}, bytes: include_bytes!({path:?}) }},\n"
        ));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("catalog.rs"),
        source,
    )
    .expect("write embedded catalog");
}
