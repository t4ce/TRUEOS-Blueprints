//! Pinned native boundaries for XERJ's display, network and file clients.
use std::path::{Path, PathBuf};

pub(crate) fn patches(
    app: &Path,
    work: &Path,
    selected: &[crate::CratePatch],
) -> Result<Vec<crate::CratePatch>, String> {
    let packages = crate::lock_packages(&crate::app_lockfile(app))?;
    let mut patches = Vec::new();
    for (name, version) in [
        ("console", "0.15.11"),
        ("socks", "0.3.4"),
        ("zip", "9.1.3"),
        ("filetime", "0.2.29"),
        ("hf-hub", "0.4.3"),
        ("tar", "0.4.46"),
        ("ignore", "0.4.32"),
        ("fs2", "0.4.3"),
    ] {
        if selected.iter().any(|patch| patch.name == name)
            || !packages.iter().any(|package| {
                package.name == name
                    && package
                        .source
                        .as_deref()
                        .is_some_and(|source| source.starts_with("registry+"))
            })
        {
            continue;
        }
        if !packages
            .iter()
            .any(|package| package.name == name && package.version == version)
        {
            return Err(format!(
                "native XERJ dependency requires pinned {name} {version}"
            ));
        }
        let source = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
            .ok_or_else(|| format!("missing original native XERJ dependency {name}-{version}"))?;
        patches.push(crate::CratePatch::new(
            name,
            stage(work, &source, name, version)?,
        ));
    }
    Ok(patches)
}

fn stage(work: &Path, source: &Path, name: &str, version: &str) -> Result<PathBuf, String> {
    if crate::package_name(&source.join("Cargo.toml"))? != name
        || crate::package_version(&source.join("Cargo.toml"))?.as_deref() != Some(version)
    {
        return Err(format!(
            "native XERJ provider requires original {name} {version}"
        ));
    }
    if name == "tar" {
        return crate::codex_native_archive_overlay::stage(work, source, name, version);
    }
    if name == "ignore" {
        return crate::codex_native_data_overlay::stage_ignore_version(work, source, version);
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("{name}-{version}-trueos-xerj"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    match name {
        "console" => {
            for (path, item) in [
                ("src/lib.rs", "mod unix_term;"),
                ("src/term.rs", "pub(crate) use crate::unix_term::*;"),
            ] {
                edit(&staged, path, |text| {
                    replace(
                        text,
                        &format!("#[cfg(all(unix, not(target_arch = \"wasm32\")))]\n{item}"),
                        &format!(
                            "#[cfg(all(unix, not(target_arch = \"wasm32\"), not(target_os = \"trueos\")))]\n{item}"
                        ),
                        1,
                    )
                })?;
            }
            edit(&staged, "src/lib.rs", |text| {
                text.push_str("\n#[cfg(target_os = \"trueos\")]\nmod trueos_term;\n");
                Ok(())
            })?;
            edit(&staged, "src/term.rs", |text| {
                text.push_str(
                    r#"
#[cfg(target_os = "trueos")]
pub(crate) use crate::trueos_term::*;
#[cfg(target_os = "trueos")]
impl Term {
    pub(crate) fn native_is_terminal(&self) -> bool {
        use std::io::IsTerminal;
        match self.inner.target {
            TermTarget::Stdout => io::stdout().is_terminal(),
            TermTarget::Stderr => io::stderr().is_terminal(),
            TermTarget::ReadWritePair(_) => false,
        }
    }
}
"#,
                );
                Ok(())
            })?;
            std::fs::write(
                staged.join("src/trueos_term.rs"),
                include_str!("xerj_console_trueos.rs"),
            )
            .map_err(|e| e.to_string())?;
        }
        "socks" => {
            edit(&staged, "src/writev.rs", |text| {
                replace(
                    text,
                    "#[cfg(unix)]",
                    "#[cfg(all(unix, not(target_os = \"trueos\")))]",
                    1,
                )?;
                text.push_str("\n#[cfg(target_os = \"trueos\")]\n#[path = \"trueos_writev.rs\"]\nmod native;\n");
                Ok(())
            })?;
            std::fs::write(
                staged.join("src/trueos_writev.rs"),
                include_str!("xerj_socks_trueos.rs"),
            )
            .map_err(|e| e.to_string())?;
        }
        "zip" => {
            // Native extraction inherits destination access keys. Keep foreign
            // metadata readable without translating its modes into native grants.
            for (path, count) in [("src/read/extract.rs", 9), ("src/read/stream.rs", 3)] {
                edit(&staged, path, |text| {
                    replace(
                        text,
                        "#[cfg(unix)]",
                        "#[cfg(all(unix, not(target_os = \"trueos\")))]",
                        count,
                    )
                })?;
            }
            edit(&staged, "src/read/extract.rs", |text| {
                text.push_str(
                    r#"
#[cfg(target_os = "trueos")]
pub(crate) fn make_symlink_impl<T>(_: &Path, _: &str, _: &IndexMap<Box<[u8]>, T>) -> ZipResult<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "native files have no symbolic-link contract").into())
}
"#,
                );
                Ok(())
            })?;
            edit(&staged, "src/read/stream.rs", |text| {
                // Preserve 9.1's real alias detection with TRUEOSFS identity.
                replace(
                    text,
                    "    #[cfg(all(unix, not(target_os = \"trueos\")))]\n    {\n        use std::os::unix::fs::MetadataExt;",
                    "    #[cfg(target_os = \"trueos\")]\n    {\n        let metadata = std::fs::metadata(path)?;\n        Ok(metadata.file_id().map(|id| (u64::from(metadata.root_id()), id)))\n    }\n    #[cfg(all(unix, not(target_os = \"trueos\")))]\n    {\n        use std::os::unix::fs::MetadataExt;",
                    1,
                )?;
                replace(
                    text,
                    "        #[cfg(not(any(unix, windows)))]\n        let created = std::fs::create_dir(&staging);",
                    "        #[cfg(any(target_os = \"trueos\", not(any(unix, windows))))]\n        let created = std::fs::create_dir(&staging);",
                    1,
                )
            })?;
        }
        "filetime" => {
            // Reuse the native archive provider's explicit unsupported contract
            // for mutable Unix times; reading uses real std metadata timestamps.
            edit(&staged, "src/lib.rs", |text| {
                replace(
                    text,
                    "    if #[cfg(target_os = \"redox\")] {",
                    "    if #[cfg(target_os = \"trueos\")] {\n        #[path = \"trueos.rs\"]\n        mod imp;\n    } else if #[cfg(target_os = \"redox\")] {",
                    1,
                )
            })?;
            std::fs::write(
                staged.join("src/trueos.rs"),
                format!(
                    "{}\n// filetime 0.2.29 delegates regular-file updates to std::File::set_times.\n// Preserve its read-only then write-only open policy without descriptor conversion.\npub fn open(path: &Path) -> io::Result<File> {{\n    File::open(path).or_else(|_| fs::OpenOptions::new().write(true).open(path))\n}}\n",
                    include_str!("filetime_trueos.rs")
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        "hf-hub" => {
            edit(&staged, "src/api/sync.rs", |text| {
                replace(
                    text,
                    "\n#[cfg(target_family = \"unix\")]",
                    "\n#[cfg(all(target_family = \"unix\", not(target_os = \"trueos\")))]",
                    2,
                )?;
                text.push_str(
                    "\n#[cfg(target_os = \"trueos\")]\n#[path = \"trueos_file_lock.rs\"]\nmod native;\n#[cfg(target_os = \"trueos\")]\nuse native::{lock, unlock};\n",
                );
                Ok(())
            })?;
            std::fs::write(
                staged.join("src/api/trueos_file_lock.rs"),
                include_str!("xerj_hf_hub_file_lock.rs"),
            )
            .map_err(|e| e.to_string())?;
        }
        "fs2" => {
            edit(&staged, "src/lib.rs", |text| {
                replace(
                    text,
                    "#[cfg(unix)]",
                    "#[cfg(all(unix, not(target_os = \"trueos\")))]",
                    2,
                )?;
                text.push_str(
                    "\n#[cfg(target_os = \"trueos\")]\n#[path = \"trueos.rs\"]\nmod native;\n#[cfg(target_os = \"trueos\")]\nuse native as sys;\n",
                );
                Ok(())
            })?;
            std::fs::write(
                staged.join("src/trueos.rs"),
                include_str!("xerj_fs2_trueos.rs"),
            )
            .map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("unsupported native XERJ dependency {name}")),
    }
    Ok(staged)
}

fn edit(
    root: &Path,
    relative: &str,
    change: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let path = root.join(relative);
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    change(&mut text)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn replace(text: &mut String, before: &str, after: &str, count: usize) -> Result<(), String> {
    if text.matches(before).count() != count {
        return Err(format!(
            "pinned native XERJ dependency boundary drifted: {before}"
        ));
    }
    *text = text.replace(before, after);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_dependency_overlays_stage_original_registry_sources() {
        let work =
            std::env::temp_dir().join(format!("xerj-native-dependencies-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        for (name, version) in [
            ("console", "0.15.11"),
            ("socks", "0.3.4"),
            ("zip", "9.1.3"),
            ("filetime", "0.2.29"),
            ("hf-hub", "0.4.3"),
            ("tar", "0.4.46"),
            ("ignore", "0.4.32"),
            ("fs2", "0.4.3"),
        ] {
            let source = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
                .expect("cached original crate");
            let staged = stage(&work, &source, name, version).unwrap();
            assert!(staged.join("Cargo.toml").is_file());
        }
        std::fs::remove_dir_all(work).unwrap();
    }
    mod hf_hub {
        include!("xerj_hf_hub_file_lock.rs");

        #[test]
        fn native_file_lock_excludes_contenders_and_releases() {
            let path = std::env::temp_dir().join(format!("xerj-hf-lock-{}", std::process::id()));
            let first = std::fs::File::create(&path).unwrap();
            let contender = std::fs::File::options()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert_eq!(lock(&first), 0);
            assert_ne!(lock(&contender), 0);
            assert_eq!(unlock(&first), 0);
            assert_eq!(lock(&contender), 0);
            assert_eq!(unlock(&contender), 0);
            drop(first);
            drop(contender);
            std::fs::remove_file(path).unwrap();
        }
    }
    mod fs2 {
        #[derive(Debug)]
        struct FsStats;
        mod native {
            include!("xerj_fs2_trueos.rs");
        }

        #[test]
        fn shared_native_locks_exclude_writers_and_release() {
            let path = std::env::temp_dir().join(format!("xerj-fs2-lock-{}", std::process::id()));
            let first = std::fs::File::create(&path).unwrap();
            let second = std::fs::File::open(&path).unwrap();
            let contender = std::fs::File::options()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            native::lock_shared(&first).unwrap();
            native::try_lock_shared(&second).unwrap();
            assert_eq!(
                native::try_lock_exclusive(&contender).unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            native::unlock(&first).unwrap();
            native::unlock(&second).unwrap();
            native::lock_exclusive(&contender).unwrap();
            native::unlock(&contender).unwrap();
            assert_eq!(
                native::allocated_size(&first).unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
            assert_eq!(
                native::allocate(&first, 4096).unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
            assert_eq!(
                native::statvfs(&path).unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
            drop(first);
            drop(second);
            drop(contender);
            std::fs::remove_file(path).unwrap();
        }

        #[test]
        fn duplicate_preserves_the_shared_file_cursor() {
            use std::io::{Read, Seek, SeekFrom, Write};
            let path = std::env::temp_dir().join(format!("xerj-fs2-clone-{}", std::process::id()));
            let mut file = std::fs::File::options()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            file.write_all(b"abcd").unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            let mut duplicate = native::duplicate(&file).unwrap();
            let mut bytes = [0; 2];
            duplicate.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"ab");
            file.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"cd");
            drop(duplicate);
            drop(file);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn custom_sources_are_not_replaced_by_matching_registry_names() {
        let work =
            std::env::temp_dir().join(format!("xerj-native-source-guards-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(
            work.join("Cargo.toml"),
            "[package]\nname=\"source-guards\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            work.join("Cargo.lock"),
            r#"
version = 4
[[package]]
name = "socks"
version = "0.3.4"
[[package]]
name = "console"
version = "0.15.11"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
        )
        .unwrap();
        let selected = [crate::CratePatch::new(
            "console",
            work.join("custom-console"),
        )];
        assert!(
            patches(&work, &work.join("staged"), &selected)
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(work).unwrap();
    }
    mod socks {
        use std::{io, net::UdpSocket};
        pub trait WritevExt {
            fn writev(&self, bufs: [&[u8]; 2]) -> io::Result<usize>;
            fn readv(&self, bufs: [&mut [u8]; 2]) -> io::Result<usize>;
        }
        mod native {
            include!("xerj_socks_trueos.rs");
        }
        #[test]
        fn gather_scatter_preserves_datagram_boundaries() {
            let left = UdpSocket::bind("127.0.0.1:0").unwrap();
            let right = UdpSocket::bind("127.0.0.1:0").unwrap();
            left.connect(right.local_addr().unwrap()).unwrap();
            right.connect(left.local_addr().unwrap()).unwrap();
            right
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            assert_eq!(left.writev([b"ab", b"cdef"]).unwrap(), 6);
            let (mut prefix, mut payload) = ([0; 2], [0; 4]);
            assert_eq!(right.readv([&mut prefix, &mut payload]).unwrap(), 6);
            assert_eq!(&prefix, b"ab");
            assert_eq!(&payload, b"cdef");
            right.set_nonblocking(true).unwrap();
            assert_eq!(
                right.recv(&mut payload).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
    }
}
