//! Archive formats keep their foreign metadata; native files inherit keys.
//! Explicit requests to copy Unix ownership, modes or timestamps fail.
use std::path::{Path, PathBuf};

pub(crate) const PROVIDERS: &[(&str, &str)] =
    &[("tar", "0.4.45"), ("filetime", "0.2.27"), ("zip", "9.0.0")];

pub(crate) fn stage(
    work: &Path,
    source: &Path,
    name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    if !PROVIDERS.contains(&(name, version))
        || crate::package_name(&source.join("Cargo.toml"))? != name
        || crate::package_version(&source.join("Cargo.toml"))?.as_deref() != Some(version)
    {
        return Err(format!(
            "native archive provider requires pinned {name} {version}"
        ));
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("{name}-{version}-trueos"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    match name {
        "filetime" => {
            edit(&staged, "src/lib.rs", |s| {
                replace(
                    s,
                    "    if #[cfg(target_os = \"redox\")] {",
                    "    if #[cfg(target_os = \"trueos\")] {\n        #[path = \"trueos.rs\"]\n        mod imp;\n    } else if #[cfg(target_os = \"redox\")] {",
                    1,
                )
            })?;
            std::fs::write(
                staged.join("src/trueos.rs"),
                include_str!("filetime_trueos.rs"),
            )
            .map_err(|e| e.to_string())?;
        }
        "tar" => {
            edit(&staged, "Cargo.toml", |s| {
                replace(
                    s,
                    "[target.\"cfg(unix)\".dependencies.xattr]",
                    "[target.'cfg(all(unix, not(target_os = \"trueos\")))'.dependencies.xattr]",
                    1,
                )
            })?;
            edit(&staged, "src/header.rs", |s| {
                replace(
                    s,
                    "    #[cfg(all(unix, not(target_arch = \"wasm32\")))]\n    fn fill_platform_from",
                    "    #[cfg(all(unix, not(target_arch = \"wasm32\"), not(target_os = \"trueos\")))]\n    fn fill_platform_from",
                    1,
                )?;
                // Put the platform method inside Header's existing impl.
                replace(
                    s,
                    "    fn fill_from(&mut self, meta: &fs::Metadata, mode: HeaderMode) {",
                    r#"    #[cfg(target_os = "trueos")]
    fn fill_platform_from(&mut self, meta: &fs::Metadata, mode: HeaderMode) {
        assert!(matches!(mode, HeaderMode::Deterministic),
            "native files have no complete Unix metadata; use deterministic archive metadata");
        // Canonical archive fields describe the portable package, never a
        // native user ID, access key, executable capability or filesystem time.
        self.set_uid(0);
        self.set_gid(0);
        self.set_mtime(DETERMINISTIC_TIMESTAMP);
        self.set_mode(if meta.is_dir() { 0o755 } else { 0o644 });
        self.set_entry_type(if meta.is_dir() { EntryType::dir() } else { EntryType::file() });
    }

    fn fill_from(&mut self, meta: &fs::Metadata, mode: HeaderMode) {"#,
                    1,
                )
            })?;
            edit(&staged, "src/builder.rs", |s| {
                replace(
                    s,
                    "mode: HeaderMode::Complete,",
                    "mode: if cfg!(target_os = \"trueos\") { HeaderMode::Deterministic } else { HeaderMode::Complete },",
                    1,
                )?;
                replace(
                    s,
                    "#[cfg(unix)]",
                    "#[cfg(all(unix, not(target_os = \"trueos\")))]",
                    3,
                )?;
                replace(
                    s,
                    "#[cfg(not(unix))]",
                    "#[cfg(any(target_os = \"trueos\", not(unix)))]",
                    1,
                )?;
                replace(
                    s,
                    "    let stat = file.metadata()?;\n    let mut header = Header::new_gnu();",
                    "    native_metadata_mode(options.mode)?;\n    let stat = file.metadata()?;\n    let mut header = Header::new_gnu();",
                    1,
                )?;
                replace(
                    s,
                    "    let mut header = Header::new_gnu();\n\n    prepare_header_path(dst, &mut header, path)?;\n    header.set_metadata_in_mode(meta, mode);",
                    "    native_metadata_mode(mode)?;\n    let mut header = Header::new_gnu();\n\n    prepare_header_path(dst, &mut header, path)?;\n    header.set_metadata_in_mode(meta, mode);",
                    1,
                )?;
                // Test injection can select TRUEOS alongside a host cfg, so
                // make the native no-sparse-syscall branch explicit too.
                replace(
                    s,
                    "#[cfg(any(target_os = \"android\", target_os = \"freebsd\", target_os = \"linux\"))]",
                    "#[cfg(all(not(target_os = \"trueos\"), any(target_os = \"android\", target_os = \"freebsd\", target_os = \"linux\")))]",
                    2,
                )?;
                replace(
                    s,
                    "#[cfg(not(any(target_os = \"android\", target_os = \"freebsd\", target_os = \"linux\")))]",
                    "#[cfg(any(target_os = \"trueos\", not(any(target_os = \"android\", target_os = \"freebsd\", target_os = \"linux\"))))]",
                    1,
                )?;
                s.push_str(r#"
fn native_metadata_mode(mode: HeaderMode) -> io::Result<()> {
    if cfg!(target_os = "trueos") && matches!(mode, HeaderMode::Complete) {
        return Err(io::Error::new(io::ErrorKind::Unsupported,
            "native files have no Unix ownership, permission or timestamp metadata; select deterministic archive metadata"));
    }
    Ok(())
}
"#);
                Ok(())
            })?;
            edit(&staged, "src/archive.rs", |s| {
                replace(
                    s,
                    "preserve_mtime: true,",
                    "preserve_mtime: !cfg!(target_os = \"trueos\"),",
                    1,
                )
            })?;
            edit(&staged, "src/entry.rs", |s| {
                replace(
                    s,
                    "    fn unpack(&mut self, target_base: Option<&Path>, dst: &Path) -> io::Result<Unpacked> {",
                    r#"    fn unpack(&mut self, target_base: Option<&Path>, dst: &Path) -> io::Result<Unpacked> {
        // Reject explicit foreign metadata preservation before writing data.
        #[cfg(target_os = "trueos")]
        if self.preserve_permissions || self.preserve_ownerships || self.preserve_mtime || self.unpack_xattrs || self.mask != 0 {
            return Err(io::Error::new(io::ErrorKind::Unsupported,
                "native extraction inherits access keys; Unix metadata preservation is unavailable"));
        }
        #[cfg(target_os = "trueos")]
        if self.header.entry_type().is_symlink() || self.header.entry_type().is_hard_link() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "native files have no symlink or hard-link contract"));
        }
        #[cfg(target_os = "trueos")]
        if self.header.entry_type().is_character_special() || self.header.entry_type().is_block_special() || self.header.entry_type().is_fifo() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "native archive extraction cannot create Unix device or FIFO objects"));
        }"#,
                    1,
                )?;
                replace(
                    s,
                    "#[cfg(all(unix, not(target_arch = \"wasm32\")))]",
                    "#[cfg(all(unix, not(target_arch = \"wasm32\"), not(target_os = \"trueos\")))]",
                    3,
                )?;
                replace(
                    s,
                    "            #[cfg(target_arch = \"wasm32\")]\n            #[allow(unused_variables)]\n            fn symlink",
                    "            #[cfg(target_os = \"trueos\")]\n            fn symlink(_: &Path, _: &Path) -> io::Result<()> { Err(io::Error::new(io::ErrorKind::Unsupported, \"native files have no symlink contract\")) }\n            #[cfg(target_arch = \"wasm32\")]\n            #[allow(unused_variables)]\n            fn symlink",
                    1,
                )?;
                replace(
                    s,
                    "#[cfg(all(unix, not(target_arch = \"wasm32\"), feature = \"xattr\"))]",
                    "#[cfg(all(unix, not(target_arch = \"wasm32\"), not(target_os = \"trueos\"), feature = \"xattr\"))]",
                    1,
                )?;
                replace(
                    s,
                    "#[cfg(any(windows, not(feature = \"xattr\"), target_arch = \"wasm32\"))]",
                    "#[cfg(any(target_os = \"trueos\", windows, not(feature = \"xattr\"), target_arch = \"wasm32\"))]",
                    1,
                )?;
                replace(
                    s,
                    "        fn set_perms(\n",
                    r#"        #[cfg(target_os = "trueos")]
        fn _set_ownerships(_: &Path, _: &Option<&mut std::fs::File>, _: u64, _: u64) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "native files use access keys, not Unix ownership"))
        }
        #[cfg(target_os = "trueos")]
        fn _set_perms(_: &Path, _: Option<&mut std::fs::File>, _: u32, mask: u32, preserve: bool) -> io::Result<()> {
            if preserve || mask != 0 {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "native files use inherited access keys"));
            }
            Ok(())
        }
        fn set_perms(
"#,
                    1,
                )
            })?;
        }
        "zip" => {
            // Keep Unix byte paths and link validation, but select the ordinary
            // non-Unix metadata behavior. Extraction uses native key inheritance.
            for relative in ["src/read/extract.rs", "src/read/stream.rs"] {
                edit(&staged, relative, |s| {
                    let n = s.matches("#[cfg(unix)]").count();
                    let expected = if relative == "src/read/extract.rs" {
                        7
                    } else {
                        4
                    };
                    if n != expected {
                        return Err(format!("pinned ZIP metadata selectors drifted: {relative}"));
                    }
                    *s = s.replace(
                        "#[cfg(unix)]",
                        "#[cfg(all(unix, not(target_os = \"trueos\")))]",
                    );
                    Ok(())
                })?;
            }
            edit(&staged, "src/read/extract.rs", |s| {
                s.push_str(r#"
// Native extraction inherits destination access keys. Unix archive fields
// remain readable, but cannot create native grants or symbolic links.
#[cfg(target_os = "trueos")]
pub(crate) fn make_symlink_impl<T>(_: &Path, _: &str, _: &IndexMap<Box<[u8]>, T>) -> ZipResult<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "native files have no symbolic-link contract").into())
}
"#);
                Ok(())
            })?;
        }
        _ => unreachable!(),
    }
    Ok(staged)
}
fn edit(
    root: &Path,
    relative: &str,
    f: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let path = root.join(relative);
    let mut source = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    f(&mut source)?;
    std::fs::write(path, source).map_err(|e| e.to_string())
}
fn replace(text: &mut String, before: &str, after: &str, count: usize) -> Result<(), String> {
    if text.matches(before).count() != count {
        return Err(format!("pinned archive boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
