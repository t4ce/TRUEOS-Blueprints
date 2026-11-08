//! Exercise the complete production selector, including later legacy stages.
use super::*;

#[test]
fn final_openssl_stage_preserves_native_boundaries_and_other_app_policy() {
    let root =
        std::env::temp_dir().join(format!("native-openssl-final-stage-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for name in ["xerj-smoke", "ordinary-openssl-app"] {
        let app = root.join(name);
        fs::create_dir_all(app.join("src")).unwrap();
        let manifest = app.join("Cargo.toml");
        fs::write(
            &manifest,
            format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"),
        )
        .unwrap();
        fs::write(app.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            app.join("Cargo.lock"),
            r#"
version = 4
[[package]]
name = "openssl-src"
version = "300.6.1+3.6.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "openssl-sys"
version = "0.9.117"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
        )
        .unwrap();
        let settings = resolve_build_settings(&app, &manifest, &BuildTarget::Package).unwrap();
        let patches =
            source_overlay_patches(&app, &manifest, &app.join("work"), &settings).unwrap();
        let source = &patches
            .iter()
            .find(|patch| patch.name == "openssl-src")
            .unwrap()
            .path;
        let build = fs::read_to_string(source.join("src/lib.rs")).unwrap();
        let header = source.join("openssl/include/internal/trueos_native.h");
        if name == "xerj-smoke" {
            assert!(
                header.is_file(),
                "later stages discarded the native C boundary"
            );
            assert!(build.contains("--with-rand-seed=getrandom"));
            assert!(build.contains("-include{}"));
            assert!(build.contains("include/internal/trueos_native.h"));
            let header = fs::read_to_string(header).unwrap();
            assert!(header.contains("ossl_trueos_"));
            let random = fs::read_to_string(
                source.join("openssl/providers/implementations/rands/seeding/rand_unix.c"),
            )
            .unwrap();
            assert!(random.contains("OPENSSL_SYS_TRUEOS"));
            assert!(random.contains("ossl_trueos_getentropy"));
            assert!(random.contains("!defined(OPENSSL_SYS_TRUEOS) && defined(__linux)"));
            let memory = fs::read_to_string(source.join("openssl/crypto/mem.c")).unwrap();
            assert!(memory.contains("!defined(OPENSSL_SYS_TRUEOS)"));
            let store = fs::read_to_string(
                source.join("openssl/providers/implementations/storemgmt/file_store.c.in"),
            )
            .unwrap();
            assert!(store.contains("Native file URI loading is unavailable"));
            let sys = &patches
                .iter()
                .find(|patch| patch.name == "openssl-sys")
                .unwrap()
                .path;
            let module = fs::read_to_string(sys.join("src/lib.rs")).unwrap();
            assert!(module.contains("native_openssl_boundary"));
            let boundary = fs::read_to_string(sys.join("src/native_openssl_boundary.rs")).unwrap();
            assert!(boundary.contains("trueos_cabi_random_bytes_v1"));
        } else {
            assert!(
                !header.exists(),
                "native XERJ policy changed another app's provider"
            );
            assert!(build.contains("--with-rand-seed=rdcpu"));
        }
    }
    fs::remove_dir_all(root).unwrap();
}
