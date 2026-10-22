//! rustc file loader backed by an immutable database revision.
//!
//! Source and asset reads under a private virtual root are answered from
//! memory. Everything else (the embedded sysroot, target JSON, and metadata)
//! is delegated to rustc's normal filesystem loader.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_span::source_map::{FileLoader, RealFileLoader};

const VIRTUAL_ROOT: &str = "/__trueos_source_db__";
const DEFAULT_ENTRY_CANDIDATES: &[&str] = &["src/main.rs", "main.rs", "src/lib.rs", "lib.rs"];

pub struct SourceFileLoader {
    files: BTreeMap<PathBuf, Arc<[u8]>>,
    root: PathBuf,
    real: RealFileLoader,
}

impl SourceFileLoader {
    pub fn new(
        project: &str,
        revision: i64,
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Self, String> {
        let project = safe_component(project);
        let root = PathBuf::from(VIRTUAL_ROOT).join(format!("{project}-r{revision}"));
        let mut mapped = BTreeMap::new();
        for (relative, bytes) in files {
            let relative = normalized_relative_path(relative.as_str())?;
            let path = root.join(relative);
            if mapped.insert(path.clone(), Arc::from(bytes)).is_some() {
                return Err(format!(
                    "database revision contains duplicate source path {}",
                    path.display()
                ));
            }
        }
        if mapped.is_empty() {
            return Err(String::from("database revision contains no source files"));
        }
        Ok(Self {
            files: mapped,
            root,
            real: RealFileLoader,
        })
    }

    pub fn entry_path(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        if let Some(requested) = requested {
            let relative = normalized_relative_path(requested)?;
            let entry = self.root.join(relative);
            return self
                .files
                .contains_key(&entry)
                .then_some(entry)
                .ok_or_else(|| format!("database revision has no entry `{requested}`"));
        }

        for candidate in DEFAULT_ENTRY_CANDIDATES {
            let path = self.root.join(candidate);
            if self.files.contains_key(&path) {
                return Ok(path);
            }
        }
        Err(format!(
            "database revision has no default Rust entry; tried {}",
            DEFAULT_ENTRY_CANDIDATES.join(", ")
        ))
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

impl FileLoader for SourceFileLoader {
    fn file_exists(&self, path: &Path) -> bool {
        if is_virtual_path(path) {
            let exists = self.files.contains_key(path);
            println!(
                "TRUEOS rustc min: database source exists path={} result={exists}",
                path.display()
            );
            exists
        } else {
            self.real.file_exists(path)
        }
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        let Some(bytes) = self.files.get(path) else {
            if is_virtual_path(path) {
                return Err(virtual_not_found(path));
            }
            return self.real.read_file(path);
        };
        println!(
            "TRUEOS rustc min: database source read path={} bytes={}",
            path.display(),
            bytes.len()
        );
        String::from_utf8(bytes.to_vec()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("database source {} is not UTF-8: {error}", path.display()),
            )
        })
    }

    fn read_binary_file(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        if let Some(bytes) = self.files.get(path) {
            return Ok(bytes.clone());
        }
        if is_virtual_path(path) {
            return Err(virtual_not_found(path));
        }
        self.real.read_binary_file(path)
    }

    fn current_directory(&self) -> io::Result<PathBuf> {
        self.real.current_directory()
    }
}

fn is_virtual_path(path: &Path) -> bool {
    path.starts_with(VIRTUAL_ROOT)
}

fn virtual_not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("database revision has no source {}", path.display()),
    )
}

fn safe_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            out.push((byte as char).to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        String::from("project")
    } else {
        out.to_owned()
    }
}

pub fn normalized_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.contains('\\') || value.as_bytes().contains(&0) {
        return Err(format!("unsafe source path `{value}`"));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(format!("absolute source path is not allowed: `{value}`"));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(component) => normalized.push(component),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(format!("unsafe source path `{value}`"));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(format!("empty source path is not allowed: `{value}`"));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_escape_the_virtual_root() {
        for path in [
            "",
            "/main.rs",
            "../main.rs",
            "src/../../main.rs",
            r"src\main.rs",
        ] {
            assert!(normalized_relative_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn serves_text_and_binary_files_from_memory() {
        let loader = SourceFileLoader::new(
            "demo",
            3,
            [
                ("src/main.rs".to_owned(), b"fn main() {}".to_vec()),
                ("assets/icon.bin".to_owned(), vec![0, 159, 255]),
            ],
        )
        .unwrap();
        let entry = loader.entry_path(None).unwrap();
        assert_eq!(loader.read_file(&entry).unwrap(), "fn main() {}");
        assert_eq!(
            loader
                .read_binary_file(&loader.root().join("assets/icon.bin"))
                .unwrap()
                .as_ref(),
            &[0, 159, 255]
        );
        let missing = loader.root().join("src/missing.rs");
        assert!(!loader.file_exists(&missing));
        assert_eq!(
            loader.read_file(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
