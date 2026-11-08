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

pub(crate) fn stage_gix_fs(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "gix-fs", "0.24.0")?;
    let staged = prepare(work, source, "gix-fs-0.24.0-trueos")?;
    let path = staged.join("src/capabilities.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg(all(unix, not(target_os = \"macos\")))]",
        "#[cfg(all(unix, not(any(target_os = \"macos\", target_os = \"trueos\"))))]",
        1,
    )?;
    replace(
        &mut text,
        "    #[cfg(unix)]",
        "    #[cfg(all(unix, not(target_os = \"trueos\")))]",
        1,
    )?;
    replace(
        &mut text,
        "    #[cfg(not(unix))]",
        "    #[cfg(any(target_os = \"trueos\", not(unix)))]",
        1,
    )?;
    replace(
        &mut text,
        "    fn probe_symlink(root: &Path) -> std::io::Result<bool> {",
        "    #[cfg(not(target_os = \"trueos\"))]\n    fn probe_symlink(root: &Path) -> std::io::Result<bool> {",
        1,
    )?;
    text.push_str(r#"
// Native files have access-key policy and no Unix executable or symlink bits.
#[cfg(target_os = "trueos")]
impl Default for Capabilities {
    fn default() -> Self {
        Self { precompose_unicode: false, ignore_case: false, executable_bit: false, symlink: false }
    }
}
#[cfg(target_os = "trueos")]
impl Capabilities {
    fn probe_symlink(_root: &Path) -> std::io::Result<bool> { Ok(false) }
}
"#);
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    let path = staged.join("src/lib.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg(unix)]",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]",
        3,
    )?;
    replace(
        &mut text,
        "#[cfg(not(unix))]",
        "#[cfg(any(target_os = \"trueos\", not(unix)))]",
        3,
    )?;
    replace(
        &mut text,
        r#"    #[cfg(all(unix, not(target_os = "trueos")))]
    if shared_repository_permissions != 0 {"#,
        r#"    #[cfg(target_os = "trueos")]
    if shared_repository_permissions != 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported,
            "Git Unix shared-repository permissions have no native access-key equivalent"));
    }
    #[cfg(all(unix, not(target_os = "trueos")))]
    if shared_repository_permissions != 0 {"#,
        1,
    )?;
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    let path = staged.join("src/file.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "    #[cfg(unix)]",
        "    // Native filesystem objects cannot be symlinks, so opening the final\n    // component needs no Unix flag. The native SDK rejects symlink creation.\n    #[cfg(all(unix, not(target_os = \"trueos\")))]",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg_attr(not(any(unix, windows)), allow(unused_mut))]",
        "#[cfg_attr(any(target_os = \"trueos\", not(any(unix, windows))), allow(unused_mut))]",
        1,
    )?;
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_ignore(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "ignore", "0.4.25")?;
    let staged = prepare(work, source, "ignore-0.4.25-trueos")?;
    let path = staged.join("src/walk.rs");
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
            ino: ent.metadata().map_err(|err| Error::Io(err).with_path(ent.path()))?
                .file_id().ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::Unsupported,
                    "native ignored-directory entry has no stable object identity")).with_path(ent.path()))?,"#,
        1,
    )?;
    replace(
        &mut text,
        r#"        let md =
            fs::metadata(&pb).map_err(|err| Error::Io(err).with_path(&pb))?;"#,
        r#"        let md =
            fs::metadata(&pb).map_err(|err| Error::Io(err).with_path(&pb))?;
        #[cfg(target_os = "trueos")]
        let native_ino = md.file_id().ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::Unsupported,
            "native ignored path has no stable object identity")).with_path(&pb))?;"#,
        2,
    )?;
    replace(
        &mut text,
        "            ino: md.ino(),",
        "            #[cfg(not(target_os = \"trueos\"))]\n            ino: md.ino(),\n            #[cfg(target_os = \"trueos\")]\n            ino: native_ino,",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nfn device_num",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nfn device_num",
        1,
    )?;
    text.push_str("\n#[cfg(target_os = \"trueos\")]\nfn device_num<P: AsRef<Path>>(path: P) -> io::Result<u64> {\n    path.as_ref().metadata().map(|md| u64::from(md.root_id()))\n}\n");
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_file_id(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "file-id", "0.2.3")?;
    let staged = prepare(work, source, "file-id-0.2.3-trueos")?;
    let path = staged.join("src/lib.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg(target_family = \"unix\")]",
        "#[cfg(all(target_family = \"unix\", not(target_os = \"trueos\")))]",
        1,
    )?;
    text.push_str(
        r#"
/// Return the native root and stable file object identity, without descriptors.
#[cfg(target_os = "trueos")]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    let metadata = fs::metadata(path.as_ref())?;
    let file_id = metadata.file_id().ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported,
        "native metadata has no stable file identity"))?;
    Ok(FileId::new_inode(u64::from(metadata.root_id()), file_id))
}
"#,
    );
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_gix_discover(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "gix-discover", "0.57.0")?;
    let staged = prepare(work, source, "gix-discover-0.57.0-trueos")?;
    let path = staged.join("src/upwards/util.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(&mut text, "#[cfg(target_os = \"linux\")]", "#[cfg(all(target_os = \"linux\", not(target_os = \"trueos\")))]", 1)?;
    replace(
        &mut text,
        "#[cfg(all(unix, not(target_os = \"linux\")))]",
        "#[cfg(all(unix, not(any(target_os = \"linux\", target_os = \"trueos\"))))]",
        1,
    )?;
    text.push_str(
        r#"
/// Native root identity is the filesystem-boundary key for Git discovery.
#[cfg(target_os = "trueos")]
pub(crate) fn device_id(metadata: &std::fs::Metadata) -> u64 {
    u64::from(metadata.root_id())
}
"#,
    );
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_supports_color(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "supports-color", "2.1.0")?;
    let staged = prepare(work, source, "supports-color-2.1.0-trueos")?;
    let path = staged.join("src/lib.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    replace(&mut text, "    use is_terminal::*;", "    #[cfg(not(target_os = \"trueos\"))]\n    use is_terminal::*;\n    #[cfg(target_os = \"trueos\")]\n    use std::io::IsTerminal;", 1)?;
    std::fs::write(path, text).map_err(|e| e.to_string())?;
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
