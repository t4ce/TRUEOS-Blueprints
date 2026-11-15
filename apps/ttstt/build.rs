//! Seal the self-contained English/Heart bundle. Never download during Cargo.
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
use trueos_kokoro_voice::VoiceArchive;

fn checked(name: &str, len: usize, digest: &str) -> (PathBuf, Vec<u8>) {
    let path = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("models/kokoro")
        .join(name);
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing bundle asset {}: {e}; run python3 tools/prepare_bundle.py in apps/ttstt first",
            path.display()
        )
    });
    assert_eq!(bytes.len(), len, "{name}: bundle asset size mismatch");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        digest,
        "{name}: bundle asset hash mismatch"
    );
    (path.canonicalize().unwrap(), bytes)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let (model, _) = checked(
        "kokoro.kkaot",
        124_081_360,
        "b7d4b9c62f3df01f71fc2585acd60190d1809dae9a6d916fe39c86d7dd4e3217",
    );
    let (g2p, _) = checked(
        "en.g2p",
        6_691_149,
        "091347d375e494b5542202201a24a0f724738a3b18c38d56a87022970c70aa9c",
    );
    let (_, archive) = checked(
        "voices-v1.0.bin",
        28_214_398,
        "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
    );
    let voices = VoiceArchive::parse(&archive).expect("validate upstream voice archive");
    let heart = voices.lookup("af_heart").unwrap().npy_bytes();
    assert_eq!(
        format!("{:x}", Sha256::digest(heart)),
        "0212418aafafb1e9878f3300787937aa401ac937cff8c0310ffa32963d96c77b"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let voice = out.join("af_heart.npy");
    fs::write(&voice, heart).unwrap();
    // The sealed AOT constants require 64-byte alignment. Embed directly in
    // aligned static storage; no runtime file read or duplicate model copy.
    let mut generated =
        "#[repr(C, align(64))]\nstruct Aligned<const N: usize>([u8; N]);\n".to_owned();
    for (symbol, path, len) in [
        ("MODEL", model, 124_081_360),
        ("G2P", g2p, 6_691_149),
        ("HEART", voice, 522_368),
    ] {
        generated.push_str(&format!(
            "static {symbol}: Aligned<{len}> = Aligned(*include_bytes!({path:?}));\n"
        ));
    }
    fs::write(out.join("bundle.rs"), generated).unwrap();
}
