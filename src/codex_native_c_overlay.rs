//! Exact C translation-unit allocator selection for the Codex native closure.
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
pub(crate) fn stage(
    work: &Path,
    source: &Path,
    name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    if !PROVIDERS.contains(&(name, version)) {
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
        "tree-sitter-bash" => ("bindings/rust/build.rs", "c_config"),
        "bzip2-sys" => ("build.rs", "cfg"),
        _ => unreachable!(),
    };
    let mut text = std::fs::read_to_string(source.join(relative)).map_err(|e| e.to_string())?;
    let before = format!("let mut {binding} = cc::Build::new();");
    if text.matches(&before).count() != 1 {
        return Err(format!("pinned {name} C compiler boundary drifted"));
    }
    let text_options = native_text::compiler_options(name, binding);
    let selected = format!(
        "{before}\n    if std::env::var(\"CARGO_CFG_TARGET_OS\").as_deref() == Ok(\"trueos\") {{\n        {binding}.define(\"malloc\", Some(\"trueos_blueprint_c_malloc\"))\n            .define(\"calloc\", Some(\"trueos_blueprint_c_calloc\"))\n            .define(\"realloc\", Some(\"trueos_blueprint_c_realloc\"))\n            .define(\"free\", Some(\"trueos_blueprint_c_free\"));{text_options}\n    }}"
    );
    text = text.replace(&before, &selected);
    if name == "tree-sitter" {
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
        let before = r#"let (defs, headerpaths) = if cfg!(feature = "pkg-config")
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some()"#;
        if text.matches(before).count() != 1 {
            return Err("Zstd system library selector drifted".into());
        }
        text = text.replace(
            before,
            r#"let (defs, headerpaths) = if target_os != "trueos" && (cfg!(feature = "pkg-config")
        || env::var_os("ZSTD_SYS_USE_PKG_CONFIG").is_some())"#,
        );
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("{name}-{version}-trueos-c-alloc"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join(relative), text).map_err(|e| e.to_string())?;
    native_text::adapt(&staged, name)?;
    if name == "tree-sitter" {
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
