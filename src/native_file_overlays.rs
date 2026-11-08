//! Pinned source overlays for native file-object consumers.
use std::path::{Path, PathBuf};

pub(crate) fn stage_tempfile(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "tempfile", "3.27.0")?;
    let file = replacement(
        source,
        "src/file/imp/mod.rs",
        "#[cfg_attr(any(unix, target_os = \"redox\", target_os = \"wasi\"), path = \"unix.rs\")]",
        "#[cfg_attr(target_os = \"trueos\", path = \"trueos.rs\")]\n#[cfg_attr(all(not(target_os = \"trueos\"), any(unix, target_os = \"redox\", target_os = \"wasi\")), path = \"unix.rs\")]",
    )?;
    let fallback = "not(any(unix, target_os = \"redox\", target_os = \"wasi\", windows))";
    if file.matches(fallback).count() != 1 {
        return Err("pinned native tempfile fallback selector drifted".into());
    }
    let file = file.replace(
        "not(any(unix, target_os = \"redox\", target_os = \"wasi\", windows))",
        "not(any(unix, target_os = \"trueos\", target_os = \"redox\", target_os = \"wasi\", windows))",
    );
    let dir = replacement(
        source,
        "src/dir/imp/mod.rs",
        "#[cfg(unix)]",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]",
    )?
    .replace(
        "#[cfg(not(unix))]",
        "#[cfg(not(any(unix, target_os = \"trueos\")))]",
    );
    let dir = format!(
        "#[cfg(target_os = \"trueos\")]\nmod trueos;\n#[cfg(target_os = \"trueos\")]\npub use trueos::*;\n\n{dir}"
    );
    let staged = prepare(work_dir, source, "tempfile-3.27.0-trueos")?;
    write(&staged, "src/file/imp/mod.rs", &file)?;
    write(&staged, "src/dir/imp/mod.rs", &dir)?;
    write(
        &staged,
        "src/file/imp/trueos.rs",
        include_str!("tempfile_trueos.rs"),
    )?;
    write(
        &staged,
        "src/dir/imp/trueos.rs",
        include_str!("tempfile_dir_trueos.rs"),
    )?;
    Ok(staged)
}

pub(crate) fn stage_fd_lock(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "fd-lock", "4.0.4")?;
    let sys = replacement(
        source,
        "src/sys/mod.rs",
        "if #[cfg(unix)] {",
        "if #[cfg(target_os = \"trueos\")] {\n        mod trueos;\n        pub use trueos::*;\n    } else if #[cfg(unix)] {",
    )?;
    let staged = prepare(work_dir, source, "fd-lock-4.0.4-trueos")?;
    write(&staged, "src/sys/mod.rs", &sys)?;
    write(
        &staged,
        "src/sys/trueos.rs",
        include_str!("fd_lock_trueos.rs"),
    )?;
    Ok(staged)
}

pub(crate) fn stage_same_file(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "same-file", "1.0.6")?;
    let path = source.join("src/lib.rs");
    let original = std::fs::read_to_string(&path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let unix = "#[cfg(any(target_os = \"redox\", unix))]";
    let other = "#[cfg(not(any(target_os = \"redox\", unix, windows)))]";
    if original.matches(unix).count() != 4 || original.matches(other).count() != 2 {
        return Err("pinned native same-file platform selector drifted".into());
    }
    // Only the module selectors exclude native TRUEOS. The public dev()/ino()
    // compatibility accessors continue returning actual root/file identities.
    let selected = original
        .replacen(
            unix,
            "#[cfg(all(not(target_os = \"trueos\"), any(target_os = \"redox\", unix)))]",
            2,
        )
        .replace(
            other,
            "#[cfg(not(any(target_os = \"trueos\", target_os = \"redox\", unix, windows)))]",
        );
    let imports = "\nuse std::fs::File;\nuse std::io;\nuse std::path::Path;";
    if selected.matches(imports).count() != 1 {
        return Err("pinned native same-file import boundary drifted".into());
    }
    // Keep the crate's leading inner documentation and attributes before items.
    let selected = selected.replacen(
        imports,
        "\n#[cfg(target_os = \"trueos\")]\nuse self::trueos as imp;\n#[cfg(target_os = \"trueos\")]\nmod trueos;\n\nuse std::fs::File;\nuse std::io;\nuse std::path::Path;",
        1,
    );
    let staged = prepare(work_dir, source, "same-file-1.0.6-trueos")?;
    write(&staged, "src/lib.rs", &selected)?;
    write(
        &staged,
        "src/trueos.rs",
        include_str!("same_file_trueos.rs"),
    )?;
    Ok(staged)
}

fn validate(source: &Path, name: &str, version: &str) -> Result<(), String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != name
        || crate::package_version(&manifest)?.as_deref() != Some(version)
    {
        return Err(format!(
            "native file overlay requires original {name} {version}: {}",
            source.display()
        ));
    }
    Ok(())
}
fn replacement(source: &Path, relative: &str, before: &str, after: &str) -> Result<String, String> {
    let path = source.join(relative);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let expected = if relative == "src/dir/imp/mod.rs" {
        2
    } else {
        1
    };
    if text.matches(before).count() != expected {
        return Err(format!(
            "pinned native file boundary drifted in {}",
            path.display()
        ));
    }
    Ok(text.replace(before, after))
}
fn prepare(work_dir: &Path, source: &Path, name: &str) -> Result<PathBuf, String> {
    let staged = work_dir.join("source-overlay-crates").join(name);
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    Ok(staged)
}
fn write(staged: &Path, relative: &str, text: &str) -> Result<(), String> {
    let path = staged.join(relative);
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}
