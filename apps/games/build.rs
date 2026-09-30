use std::{env, fs, path::PathBuf};

const NAMES: [&str; 6] = ["pawn", "knight", "bishop", "rook", "queen", "king"];

fn main() {
    println!("cargo:rerun-if-env-changed=GAMES_CHESS_ASSETS_DIR");
    let directory = env::var_os("GAMES_CHESS_ASSETS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets")
        });
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut generated = String::from("pub const CHESS_ASSET_BYTES: [Option<&[u8]>; 6] = [\n");
    for name in NAMES {
        let path = directory.join(format!("chess_{name}.cubes"));
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_file() {
            let bytes = fs::read(&path).expect("read chess .cubes");
            assert!(
                bytes.len() >= 16 && &bytes[..4] == b"CUBE" && (bytes[4] == 1 || bytes[4] == 2),
                "invalid chess asset {}",
                path.display()
            );
            let path = fs::canonicalize(&path).expect("canonical chess asset");
            generated.push_str(&format!("    Some(include_bytes!({:?})),\n", path));
        } else {
            generated.push_str("    None,\n");
        }
    }
    generated.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("chess_assets.rs"), generated).expect("write chess asset catalog");
}
