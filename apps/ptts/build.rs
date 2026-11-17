//! Verify local build inputs and tokenize the fixed sample on the developer host.
//! Tokenization is preparation only; inference and audio decoding run on TRUEOS.
use sha2::{Digest, Sha256};
use std::{env, fs, io::Read, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=asset-sources.json");
    println!("cargo:rerun-if-changed=sample.txt");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let sources: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("asset-sources.json")).expect("asset provenance"),
    )
    .expect("valid asset provenance");
    let model = &sources["assets"]["model.safetensors"];
    let mut generated = format!(
        "pub const MODEL_BYTES: usize = {};\npub const MODEL_SHA256: &str = {:?};\n",
        model["bytes"].as_u64().expect("model size"),
        model["sha256"].as_str().expect("model SHA-256"),
    );
    generated.push_str("#[repr(C, align(64))]\nstruct Aligned<const N: usize>([u8; N]);\n");
    for (name, symbol) in [
        ("azelma.safetensors", "AZELMA"),
        ("tokenizer.json", "TOKENIZER"),
    ] {
        let path = root.join("models").join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut input = fs::File::open(&path).unwrap_or_else(|e| {
            panic!("{}: {e}; run tools/prepare_bundle.py first", path.display())
        });
        let mut hasher = Sha256::new();
        let mut count = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            let n = input.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            count += n as u64;
        }
        let expected = &sources["assets"][name];
        assert_eq!(
            Some(count),
            expected["bytes"].as_u64(),
            "{name}: size mismatch"
        );
        assert_eq!(
            format!("{:x}", hasher.finalize()),
            expected["sha256"].as_str().unwrap(),
            "{name}: hash mismatch"
        );
        let path = path.canonicalize().unwrap();
        generated.push_str(&format!(
            "static {symbol}: Aligned<{count}> = Aligned(*include_bytes!({path:?}));\n"
        ));
    }
    let text = fs::read_to_string(root.join("sample.txt")).unwrap();
    let text = text.trim();
    let tokenizer = tokenizers::Tokenizer::from_file(root.join("models/tokenizer.json")).unwrap();
    assert_eq!(tokenizer.get_vocab_size(true), 4000);
    let encoding = tokenizer.encode(text, true).unwrap();
    assert!(
        !encoding.get_ids().is_empty() && encoding.get_ids().len() <= 50,
        "fixed sample must fit one synthesis chunk"
    );
    generated.push_str(&format!(
        "pub const SAMPLE_TEXT: &str = {text:?};\npub const SAMPLE_TOKENS: &[u32] = &{:?};\n",
        encoding.get_ids()
    ));
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("bundle.rs"), generated).unwrap();
}
