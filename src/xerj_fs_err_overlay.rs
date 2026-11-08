//! Preserve fs-err's path context over native std and Tokio filesystem objects.
use std::path::{Path, PathBuf};

pub(crate) fn stage(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "fs-err"
        || crate::package_version(&manifest)?.as_deref() != Some("3.3.1")
    {
        return Err("native fs-err provider requires fs-err 3.3.1".into());
    }
    let staged = work.join("source-overlay-crates/fs-err-3.3.1-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    for (relative, count) in [
        ("src/dir.rs", 1),
        ("src/open_options.rs", 1),
        ("src/file.rs", 1),
        ("src/os.rs", 1),
        ("src/errors.rs", 16),
        ("src/tokio/open_options.rs", 1),
        ("src/tokio/dir_builder.rs", 1),
        ("src/tokio/file.rs", 1),
        ("src/tokio/mod.rs", 1),
        ("src/tokio/read_dir.rs", 1),
    ] {
        edit(&staged, relative, |s| {
            replace(s, "#[cfg(unix)]", "#[cfg(all(unix, not(target_os = \"trueos\")))]", count)
        })?;
    }
    for relative in ["src/tokio/file.rs", "src/tokio/mod.rs"] {
        edit(&staged, relative, |s| replace(
            s,
            "use std::fs::{Metadata, Permissions};",
            "#[cfg(not(target_os = \"trueos\"))]\nuse std::fs::{Metadata, Permissions};\n#[cfg(target_os = \"trueos\")]\nuse tokio::fs::{Metadata, Permissions};",
            1,
        ))?;
    }
    edit(&staged, "src/tokio/read_dir.rs", |s| replace(
        s,
        "use std::fs::{FileType, Metadata};",
        "#[cfg(not(target_os = \"trueos\"))]\nuse std::fs::{FileType, Metadata};\n#[cfg(target_os = \"trueos\")]\nuse tokio::fs::{FileType, Metadata};",
        1,
    ))?;
    edit(&staged, "src/tokio/file.rs", |s| {
        // A native Tokio file can own a different backend than std::fs::File.
        // Transfer that actual capability and its path instead of reopening a
        // pathname, inventing a Unix descriptor or claiming std type identity.
        replace(s, "    pub async fn into_std(self) -> crate::File {", "    #[cfg(not(target_os = \"trueos\"))]\n    pub async fn into_std(self) -> crate::File {", 1)?;
        replace(s, "    pub fn try_into_std(self) -> Result<crate::File, File> {", "    #[cfg(not(target_os = \"trueos\"))]\n    pub fn try_into_std(self) -> Result<crate::File, File> {", 1)?;
        replace(s, "impl From<crate::File> for File {", "#[cfg(not(target_os = \"trueos\"))]\nimpl From<crate::File> for File {", 1)?;
        s.push_str(r#"
#[cfg(target_os = "trueos")]
impl File {
    /// Transfer the actual native file capability and its associated path.
    /// Native Tokio files do not have std::fs::File type identity.
    pub async fn into_std(self) -> (impl io::Read + io::Write + io::Seek, PathBuf) {
        (self.tokio.into_std().await, self.path)
    }

    /// Transfer without waiting; failure retains this exact async file.
    pub fn try_into_std(self) -> Result<(impl io::Read + io::Write + io::Seek, PathBuf), File> {
        match self.tokio.try_into_std() {
            Ok(file) => Ok((file, self.path)),
            Err(file) => Err(File::from_parts(file, self.path)),
        }
    }
}

#[cfg(target_os = "trueos")]
impl From<crate::File> for File {
    fn from(file: crate::File) -> Self { File::from_std(file) }
}
"#);
        Ok(())
    })?;
    edit(&staged, "src/tokio/open_options.rs", |s| {
        replace(s, "impl From<std::fs::OpenOptions> for OpenOptions {", "#[cfg(not(target_os = \"trueos\"))]\nimpl From<std::fs::OpenOptions> for OpenOptions {", 1)?;
        s.push_str(r#"
#[cfg(target_os = "trueos")]
impl std::convert::TryFrom<std::fs::OpenOptions> for OpenOptions {
    type Error = io::Error;
    fn try_from(_: std::fs::OpenOptions) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported,
            "native std open options cannot be converted to Tokio options; configure native Tokio options directly"))
    }
}
"#);
        Ok(())
    })?;
    Ok(staged)
}

fn edit(root: &Path, relative: &str, f: impl FnOnce(&mut String) -> Result<(), String>) -> Result<(), String> {
    let path = root.join(relative);
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?.replace("\r\n", "\n");
    f(&mut text)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn replace(text: &mut String, before: &str, after: &str, count: usize) -> Result<(), String> {
    if text.matches(before).count() != count {
        return Err(format!("pinned fs-err boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
