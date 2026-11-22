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
    for (name, symbol) in [
        ("startthink", "STARTTHINK"),
        ("starttalk", "STARTTALK"),
        ("done", "DONE"),
    ] {
        let path = root.join("assets").join(format!("{name}.mp3"));
        println!("cargo:rerun-if-changed={}", path.display());
        let pcm = decode_cue(&path);
        let destination = out.join(format!("{name}.pcm"));
        fs::write(&destination, &pcm).expect("write cue PCM");
        generated.push_str(&format!(
            "pub static {symbol}: &[u8] = include_bytes!({destination:?});\n"
        ));
    }
    fs::write(out.join("bundle.rs"), generated).unwrap();
}

/// Decode the local notification assets on the build host. The Blueprint only
/// ships PCM: no MP3 decoder or runtime dependency is added to the speech VM.
fn decode_cue(path: &std::path::Path) -> Vec<u8> {
    use symphonia::core::{audio::SampleBuffer, errors::Error, io::MediaSourceStream, probe::Hint};
    let source = MediaSourceStream::new(
        Box::new(fs::File::open(path).expect("open cue MP3")),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut options = symphonia::core::formats::FormatOptions::default();
    options.enable_gapless = true;
    let mut format = symphonia::default::get_probe()
        .format(&hint, source, &options, &Default::default())
        .expect("probe cue MP3")
        .format;
    let track = format.default_track().expect("cue track");
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &Default::default())
        .expect("cue decoder");
    let mut pcm = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => panic!("{}: {error}", path.display()),
        };
        if packet.track_id() != id {
            continue;
        }
        let audio = decoder.decode(&packet).expect("decode cue MP3");
        assert_eq!(audio.spec().rate, 48_000, "cue must be 48 kHz");
        assert_eq!(audio.spec().channels.count(), 2, "cue must be stereo");
        let mut samples = SampleBuffer::<i16>::new(audio.capacity() as u64, *audio.spec());
        samples.copy_interleaved_ref(audio);
        pcm.extend(
            samples
                .samples()
                .iter()
                .flat_map(|sample| sample.to_le_bytes()),
        );
        assert!(pcm.len() <= 48_000 * 4 * 5, "cue exceeds five seconds");
    }
    assert!(!pcm.is_empty(), "empty cue");
    pcm
}
