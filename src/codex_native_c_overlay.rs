//! Exact C translation-unit allocator selection for native Blueprint closures.
use std::path::{Path, PathBuf};
#[path = "codex_native_c_text.rs"]
mod native_text;

pub(crate) const PROVIDERS: &[(&str, &str)] = &[
    ("zstd-sys", "2.0.16+zstd.1.5.7"),
    ("onig_sys", "69.9.1"),
    ("tree-sitter", "0.25.10"),
    ("tree-sitter-bash", "0.25.1"),
    ("bzip2-sys", "0.1.13+1.0.8"),
];
// Keep the original Codex closure fixed. Other native consumers select these
// original registry sources by dependency and exact version.
pub(crate) const ADDITIONAL_PROVIDERS: &[(&str, &str)] = &[
    ("zstd-sys", "2.1.2+zstd.1.5.7"),
    ("tree-sitter", "0.26.12"),
    ("tree-sitter-bash", "0.23.3"),
    ("tree-sitter-c", "0.23.4"),
    ("tree-sitter-c-sharp", "0.23.5"),
    ("tree-sitter-clojure-orchard", "0.2.8"),
    ("tree-sitter-cpp", "0.23.4"),
    ("tree-sitter-dart", "0.2.0"),
    ("tree-sitter-elixir", "0.3.5"),
    ("tree-sitter-erlang", "0.20.0"),
    ("tree-sitter-fortran", "0.6.0"),
    ("tree-sitter-fsharp", "0.3.11"),
    ("tree-sitter-go", "0.23.4"),
    ("tree-sitter-groovy", "0.1.2"),
    ("tree-sitter-haskell", "0.23.1"),
    ("tree-sitter-java", "0.23.5"),
    ("tree-sitter-javascript", "0.23.1"),
    ("tree-sitter-julia", "0.23.1"),
    ("tree-sitter-kotlin-ng", "1.1.0"),
    ("tree-sitter-lua", "0.5.0"),
    ("tree-sitter-matlab", "1.3.0"),
    ("tree-sitter-nix", "0.3.0"),
    ("tree-sitter-objc", "3.0.2"),
    ("tree-sitter-ocaml", "0.25.0"),
    ("tree-sitter-perl", "1.1.2"),
    ("tree-sitter-php", "0.23.11"),
    ("tree-sitter-powershell", "0.26.4"),
    ("tree-sitter-python", "0.23.6"),
    ("tree-sitter-r", "1.3.0"),
    ("tree-sitter-ruby", "0.23.1"),
    ("tree-sitter-rust", "0.23.3"),
    ("tree-sitter-scala", "0.26.2"),
    ("tree-sitter-scheme", "0.24.7"),
    ("tree-sitter-solidity", "1.2.13"),
    ("tree-sitter-swift", "0.7.3"),
    ("tree-sitter-typescript", "0.23.2"),
    ("tree-sitter-zig", "1.1.2"),
];
pub(crate) fn stage(
    work: &Path,
    source: &Path,
    name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    if !PROVIDERS.contains(&(name, version)) && !ADDITIONAL_PROVIDERS.contains(&(name, version)) {
        return Err(format!("unsupported native C provider {name} {version}"));
    }
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != name
        || crate::package_version(&manifest)?.as_deref() != Some(version)
    {
        return Err(format!(
            "native C provider requires original {name} {version}"
        ));
    }
    let (relative, binding) = match name {
        "zstd-sys" => ("build.rs", "config"),
        "onig_sys" => ("build.rs", "cc"),
        "tree-sitter" => ("binding_rust/build.rs", "config"),
        "tree-sitter-php" | "tree-sitter-typescript" => ("bindings/rust/build.rs", "config"),
        name if name.starts_with("tree-sitter-") => ("bindings/rust/build.rs", "c_config"),
        "bzip2-sys" => ("build.rs", "cfg"),
        _ => unreachable!(),
    };
    let mut text = std::fs::read_to_string(source.join(relative)).map_err(|e| e.to_string())?;
    let before = format!("let mut {binding} = cc::Build::new();");
    if text.matches(&before).count() != 1 {
        return Err(format!("pinned {name} C compiler boundary drifted"));
    }
    let mut text_options = native_text::compiler_options(name, binding);
    let native_grammar =
        name.starts_with("tree-sitter-") && ADDITIONAL_PROVIDERS.contains(&(name, version));
    if native_grammar {
        text_options.push_str(&format!(
            "\n        {binding}.include(\".\").flag(\"-include\").flag(\"trueos_native_grammar.h\");"
        ));
    }
    let selected = format!(
        "{before}\n    if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() == Ok(\"trueos\") {{\n        {binding}.define(\"malloc\", Some(\"trueos_blueprint_c_malloc\"))\n            .define(\"calloc\", Some(\"trueos_blueprint_c_calloc\"))\n            .define(\"realloc\", Some(\"trueos_blueprint_c_realloc\"))\n            .define(\"free\", Some(\"trueos_blueprint_c_free\"));{text_options}\n    }}"
    );
    text = text.replace(&before, &selected);
    if name == "zstd-sys" {
        // The native runtime has no optional ELF weak tracing provider. Use
        // Zstd's own switch to omit those hooks and their context bookkeeping.
        text = text.replace(&before, &format!("{before}\n    if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() == Ok(\"trueos\") {{ {binding}.define(\"ZSTD_NO_TRACE\", None); }}"));
    }
    if name == "tree-sitter" && version == "0.25.10" {
        let before = "let mut config = cc::Build::new();";
        text = text.replace(before, &format!("{before}\n    if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() == Ok(\"trueos\") {{ config.define(\"TRUEOS_BLUEPRINT_NATIVE_CLOCK\", None); }}"));
    }
    // A native provider must compile the selected sources; an installed host
    // library cannot silently reintroduce another allocator family.
    if name == "bzip2-sys" {
        let before = "} else if !cfg!(feature = \"static\") {";
        if text.matches(before).count() != 1 {
            return Err("bzip2 system library selector drifted".into());
        }
        text = text.replace(before, "} else if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() != Ok(\"trueos\") && !cfg!(feature = \"static\") {");
    }
    if name == "onig_sys" {
        let before = "pub fn main() {";
        if text.matches(before).count() != 1 {
            return Err("Onig main selector drifted".into());
        }
        text = text.replace(
            before,
            r#"pub fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("trueos") { compile(); return; }"#,
        );
    }
    if name == "zstd-sys" {
        let (before, after) = if version == "2.1.2+zstd.1.5.7" {
            (
                r#"let (defs, headerpaths) = if (cfg!(feature = "pkg-config")
        && !cfg!(feature = "vendored"))
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some()"#,
                r#"let (defs, headerpaths) = if target_os != "trueos" && ((cfg!(feature = "pkg-config")
        && !cfg!(feature = "vendored"))
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some())"#,
            )
        } else {
            (
                r#"let (defs, headerpaths) = if cfg!(feature = "pkg-config")
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some()"#,
                r#"let (defs, headerpaths) = if target_os != "trueos" && (cfg!(feature = "pkg-config")
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some())"#,
            )
        };
        if text.matches(before).count() != 1 {
            return Err("Zstd system library selector drifted".into());
        }
        text = text.replace(before, after);
        if version == "2.1.2+zstd.1.5.7" {
            let before = "fn main() {";
            if text.matches(before).count() != 1 {
                return Err("Zstd native feature boundary drifted".into());
            }
            text = text.replace(
                before,
                &format!(
                    r#"{before}
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("trueos")
        && (cfg!(feature = "cmake") || cfg!(feature = "zstdmt")) {{
        panic!("native Zstd requires its selected cc allocator provider without C pthread workers");
    }}"#
                ),
            );
        }
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("{name}-{version}-trueos-c-alloc"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join(relative), text).map_err(|e| e.to_string())?;
    if native_grammar {
        std::fs::write(
            staged.join("trueos_native_grammar.h"),
            include_str!("native_c_grammar.h"),
        )
        .map_err(|e| e.to_string())?;
    }
    native_text::adapt(&staged, name, version)?;
    if name == "tree-sitter" && version == "0.25.10" {
        let clock_path = staged.join("src/clock.h");
        let clock = std::fs::read_to_string(&clock_path).map_err(|e| e.to_string())?;
        let before = "#ifdef _WIN32";
        if clock.matches(before).count() != 1 {
            return Err("Tree-sitter clock provider selector drifted".into());
        }
        std::fs::write(&clock_path, clock.replace(before, "#if defined(TRUEOS_BLUEPRINT_NATIVE_CLOCK)\n#include \"trueos_native_clock.h\"\n#elif defined(_WIN32)")).map_err(|e| e.to_string())?;
        std::fs::write(
            staged.join("src/trueos_native_clock.h"),
            include_str!("tree_sitter_native_clock.h"),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(staged)
}
