//! Select native socket capabilities for exact Codex dependency versions.
use std::path::{Path, PathBuf};
fn platform_items(source: &str) -> Result<String, String> {
    let split = if source.starts_with("use ") {
        0
    } else {
        source
            .find("\nuse ")
            .map(|index| index + 1)
            .ok_or("socket provider has no pinned first item")?
    };
    let (prefix, items) = source.split_at(split);
    Ok(format!(
        "{prefix}\n#[cfg(target_os=\"trueos\")]\n#[allow(missing_docs)]\nmod trueos_native;\n#[cfg(target_os=\"trueos\")]\npub use trueos_native::*;\nmacro_rules! original_platform_items {{ ($($item:item)*) => {{ $(#[cfg(not(target_os=\"trueos\"))] $item)* }}; }}\noriginal_platform_items! {{\n{items}\n}}\n"
    ))
}
pub(crate) fn stage_socket2(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    let version = crate::package_version(&manifest)?.ok_or("Socket2 version missing")?;
    if crate::package_name(&manifest)? != "socket2"
        || !matches!(version.as_str(), "0.6.3" | "0.5.10")
    {
        return Err("native Socket2 requires pinned 0.6.3/0.5.10".into());
    }
    let text = std::fs::read_to_string(source.join("src/lib.rs")).map_err(|e| e.to_string())?;
    if !text.contains("pub use socket::Socket;") || !text.contains("pub struct Domain(") {
        return Err("pinned Socket2 root exports drifted".into());
    }
    let staged = work.join(format!(
        "source-overlay-crates/socket2-{version}-trueos-native"
    ));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/lib.rs"), platform_items(&text)?).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/trueos_native.rs"),
        include_str!("socket2_trueos.rs"),
    )
    .map_err(|e| e.to_string())?;
    Ok(staged)
}
pub(crate) fn stage_rama_udp(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "rama-udp"
        || crate::package_version(&manifest)?.as_deref() != Some("0.3.0-alpha.4")
    {
        return Err("native UDP requires rama-udp 0.3.0-alpha.4".into());
    }
    let text = std::fs::read_to_string(source.join("src/socket.rs")).map_err(|e| e.to_string())?;
    if text.matches("pub async fn bind_udp").count() != 4
        || text.matches("fn bind_socket_internal(").count() != 1
    {
        return Err("pinned Rama UDP entry points drifted".into());
    }
    let staged = work.join("source-overlay-crates/rama-udp-0.3.0-alpha.4-trueos-native");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    // socket.rs is a module file, so its sibling module needs an explicit path.
    let text = platform_items(&text)?.replace(
        "mod trueos_native;",
        "#[path=\"socket_trueos.rs\"]\nmod trueos_native;",
    );
    std::fs::write(staged.join("src/socket.rs"), text).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/socket_trueos.rs"),
        include_str!("rama_udp_trueos.rs"),
    )
    .map_err(|e| e.to_string())?;
    let mut manifest =
        std::fs::read_to_string(staged.join("Cargo.toml")).map_err(|e| e.to_string())?;
    if manifest.contains("[dependencies.serde_json]") {
        return Err("Rama UDP JSON dependency drifted".into());
    }
    manifest.push_str("\n[dependencies.serde_json]\nversion = \"1\"\n");
    std::fs::write(staged.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_rama_unix(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "rama-unix"
        || crate::package_version(&manifest)?.as_deref() != Some("0.3.0-alpha.4")
    {
        return Err("native Unix boundary requires rama-unix 0.3.0-alpha.4".into());
    }
    let staged = work.join("source-overlay-crates/rama-unix-0.3.0-alpha.4-trueos-native");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    let path = staged.join("src/unix/server/listener.rs");
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    for (signature, replacement) in [
        (
            "    pub async fn bind_path(self, path: impl AsRef<Path>) -> Result<UnixListener, io::Error> {",
            r#"    #[cfg(target_os = "trueos")]
    pub async fn bind_path(self, _path: impl AsRef<Path>) -> Result<UnixListener, io::Error> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "native networking has no Unix-domain listener provider"))
    }
    #[cfg(not(target_os = "trueos"))]
    pub async fn bind_path(self, path: impl AsRef<Path>) -> Result<UnixListener, io::Error> {"#,
        ),
        (
            "    pub fn bind_socket(\n        self,\n        socket: rama_net::socket::core::Socket,\n    ) -> Result<UnixListener, io::Error> {",
            r#"    #[cfg(target_os = "trueos")]
    pub fn bind_socket(self, _socket: rama_net::socket::core::Socket) -> Result<UnixListener, io::Error> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "native capabilities cannot become Unix-domain listeners"))
    }
    #[cfg(not(target_os = "trueos"))]
    pub fn bind_socket(
        self,
        socket: rama_net::socket::core::Socket,
    ) -> Result<UnixListener, io::Error> {"#,
        ),
    ] {
        if text.matches(signature).count() != 1 {
            return Err("pinned Rama Unix listener factories drifted".into());
        }
        text = text.replace(signature, replacement);
    }
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(staged)
}
