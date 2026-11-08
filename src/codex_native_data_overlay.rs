//! Pinned native providers for Codex's RNG, Git files and object identity.
use std::path::{Path, PathBuf};

pub(crate) fn stage_rand(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "rand", "0.8.6")?;
    let path = source.join("src/rngs/adapter/reseeding.rs");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let original = "all(unix, not(target_os = \"emscripten\"))";
    if text.matches(original).count() != 2 {
        return Err("pinned rand fork-provider selectors drifted".into());
    }
    let text = text.replace(
        original,
        "all(unix, not(any(target_os = \"emscripten\", target_os = \"trueos\")))",
    );
    let staged = prepare(work, source, "rand-0.8.6-trueos")?;
    std::fs::write(staged.join("src/rngs/adapter/reseeding.rs"), text)
        .map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_memmap(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "memmap2", "0.9.11")?;
    let mut text = std::fs::read_to_string(source.join("src/lib.rs")).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg_attr(unix, path = \"unix.rs\")]",
        "#[cfg_attr(target_os = \"trueos\", path = \"trueos.rs\")]\n#[cfg_attr(all(unix, not(target_os = \"trueos\")), path = \"unix.rs\")]",
        1,
    )?;
    // A borrowed native File stays a File; its opaque capability is never an FD.
    replace(
        &mut text,
        "#[cfg(not(any(unix, windows)))]",
        "#[cfg(any(target_os = \"trueos\", not(any(unix, windows))))]",
        3,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nuse std::os::unix::io::{AsRawFd, RawFd};",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nuse std::os::unix::io::{AsRawFd, RawFd};",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\npub struct MmapRawDescriptor",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\npub struct MmapRawDescriptor",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nimpl MmapAsRawDesc for RawFd",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nimpl MmapAsRawDesc for RawFd",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nimpl<T> MmapAsRawDesc for &T",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nimpl<T> MmapAsRawDesc for &T",
        1,
    )?;
    let staged = prepare(work, source, "memmap2-0.9.11-trueos")?;
    std::fs::write(staged.join("src/lib.rs"), text).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/trueos.rs"),
        include_str!("memmap2_trueos.rs"),
    )
    .map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_walkdir(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "walkdir", "2.5.0")?;
    let staged = prepare(work, source, "walkdir-2.5.0-trueos")?;
    let path = staged.join("src/dent.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "        use std::os::unix::fs::DirEntryExt;",
        "        #[cfg(not(target_os = \"trueos\"))]\n        use std::os::unix::fs::DirEntryExt;",
        1,
    )?;
    replace(
        &mut text,
        "        use std::os::unix::fs::MetadataExt;",
        "        #[cfg(not(target_os = \"trueos\"))]\n        use std::os::unix::fs::MetadataExt;",
        1,
    )?;
    replace(
        &mut text,
        "            ino: ent.ino(),",
        r#"            #[cfg(not(target_os = "trueos"))]
            ino: ent.ino(),
            #[cfg(target_os = "trueos")]
            ino: ent.metadata().map_err(|err| Error::from_path(depth, ent.path(), err))?
                .file_id().ok_or_else(|| Error::from_path(depth, ent.path(),
                    std::io::Error::new(std::io::ErrorKind::Unsupported, "native directory entry has no stable object identity")))?,"#,
        1,
    )?;
    replace(
        &mut text,
        "            ino: md.ino(),",
        r#"            #[cfg(not(target_os = "trueos"))]
            ino: md.ino(),
            #[cfg(target_os = "trueos")]
            ino: md.file_id().ok_or_else(|| Error::from_path(depth, pb.clone(),
                std::io::Error::new(std::io::ErrorKind::Unsupported, "native path has no stable object identity")))?,"#,
        1,
    )?;
    // Compute the fallible identity before moving the path into the entry.
    replace(
        &mut text,
        "            ino: md.file_id().ok_or_else(|| Error::from_path(depth, pb.clone(),",
        "            ino: md.file_id().ok_or_else(|| Error::from_path(depth, md_path.clone(),",
        1,
    )?;
    replace(
        &mut text,
        "        use std::os::unix::fs::MetadataExt;\n\n        let md =",
        "        use std::os::unix::fs::MetadataExt;\n\n        #[cfg(target_os = \"trueos\")]\n        let md_path = pb.clone();\n        let md =",
        1,
    )?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    let path = staged.join("src/util.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg(unix)]",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]",
        1,
    )?;
    text.push_str("\n#[cfg(target_os = \"trueos\")]\npub fn device_num<P: AsRef<Path>>(path: P) -> io::Result<u64> {\n    path.as_ref().metadata().map(|md| u64::from(md.root_id()))\n}\n");
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_gix_sec(work: &Path, source: &Path, sdk_api: &Path) -> Result<PathBuf, String> {
    validate(source, "gix-sec", "0.16.0")?;
    let staged = prepare(work, source, "gix-sec-0.16.0-trueos")?;
    let path = staged.join("src/identity.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg(all(not(windows), not(target_os = \"wasi\")))]",
        "#[cfg(all(not(windows), not(any(target_os = \"wasi\", target_os = \"trueos\"))))]",
        1,
    )?;
    text.push_str(
        "\n#[cfg(target_os = \"trueos\")]\n#[path = \"identity_trueos.rs\"]\nmod impl_;\n",
    );
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/identity_trueos.rs"),
        include_str!("gix_sec_trueos.rs"),
    )
    .map_err(|e| e.to_string())?;
    let path = staged.join("Cargo.toml");
    let mut manifest = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let api = sdk_api.canonicalize().map_err(|e| e.to_string())?;
    let api = api.to_str().ok_or("native Git SDK path is not UTF-8")?;
    manifest.push_str(&format!("\n[target.'cfg(target_os = \"trueos\")'.dependencies.trueos]\npath = {api:?}\ndefault-features = false\n"));
    std::fs::write(path, manifest).map_err(|e| e.to_string())?;
    Ok(staged)
}
fn replace(text: &mut String, before: &str, after: &str, expected: usize) -> Result<(), String> {
    if text.matches(before).count() != expected {
        return Err(format!("pinned memmap2 boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
fn validate(source: &Path, name: &str, version: &str) -> Result<(), String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != name
        || crate::package_version(&manifest)?.as_deref() != Some(version)
    {
        return Err(format!(
            "native Codex data provider requires {name} {version}"
        ));
    }
    Ok(())
}
fn prepare(work: &Path, source: &Path, name: &str) -> Result<PathBuf, String> {
    let staged = work.join("source-overlay-crates").join(name);
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    Ok(staged)
}
