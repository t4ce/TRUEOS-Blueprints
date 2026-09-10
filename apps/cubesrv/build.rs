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
    println!("cargo:rerun-if-changed=worlds");
    println!("cargo:rerun-if-changed=assets");
    let source = format!(
        "{}{}",
        catalog("worlds", "WORLDS", 27),
        catalog("assets", "ASSETS", 49)
    );
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("catalog.rs"),
        source,
    )
    .expect("write embedded catalog");
}
