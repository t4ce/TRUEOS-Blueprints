//! Dependency-selected native SQLite, using the service's audited Rust
//! allocator/mutex boundary and a native File VFS for ordinary SQLite inputs.
use std::path::{Path, PathBuf};

pub(crate) fn patches(
    app: &Path,
    work: &Path,
    selected: &[crate::CratePatch],
) -> Result<Vec<crate::CratePatch>, String> {
    let packages = crate::lock_packages(&crate::app_lockfile(app))?;
    let mut out = Vec::new();
    for (name, version) in [("libsqlite3-sys", "0.30.1"), ("rusqlite", "0.32.1")] {
        if selected.iter().any(|patch| patch.name == name)
            || !packages.iter().any(|package| {
                package.name == name
                    && package.version == version
                    && package
                        .source
                        .as_deref()
                        .is_some_and(|s| s.starts_with("registry+"))
            })
        {
            continue;
        }
        let source = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
            .ok_or_else(|| format!("missing pinned native SQLite source {name}-{version}"))?;
        out.push(crate::CratePatch::new(
            name,
            stage(work, &source, name, version)?,
        ));
    }
    Ok(out)
}
fn replace(text: &mut String, before: &str, after: &str) -> Result<(), String> {
    if text.matches(before).count() != 1 {
        return Err(format!("native SQLite provider boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
fn stage(work: &Path, original: &Path, name: &str, version: &str) -> Result<PathBuf, String> {
    if crate::package_name(&original.join("Cargo.toml"))? != name
        || crate::package_version(&original.join("Cargo.toml"))?.as_deref() != Some(version)
    {
        return Err(format!(
            "native SQLite provider requires original {name}-{version}"
        ));
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("{name}-{version}-trueos-native"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(original, &staged)?;
    if name == "libsqlite3-sys" {
        let path = staged.join("build.rs");
        let mut build = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        replace(
            &mut build,
            "        println!(\"cargo:rerun-if-env-changed=LIBSQLITE3_FLAGS\");",
            r#"
        if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("trueos") || cfg!(feature = "trueos-native-testing") {
            cfg.flag("-DSQLITE_OS_OTHER=1")
                .flag("-DSQLITE_MUTEX_APPDEF=1").flag("-DSQLITE_ZERO_MALLOC=1")
                .flag("-DSQLITE_TEMP_STORE=3").flag("-DSQLITE_OMIT_LOAD_EXTENSION=1")
                .flag("-USQLITE_ENABLE_LOAD_EXTENSION").flag("-DSQLITE_OMIT_LOCALTIME=1");
        }
        println!("cargo:rerun-if-env-changed=LIBSQLITE3_FLAGS");"#,
        )?;
        std::fs::write(path, build).map_err(|e| e.to_string())?;
        let path = staged.join("Cargo.toml");
        let mut manifest = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        replace(
            &mut manifest,
            "[features]",
            "[features]\ntrueos-native-testing = []",
        )?;
        std::fs::write(path, manifest).map_err(|e| e.to_string())?;
        let path = staged.join("src/lib.rs");
        let mut library = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        library.push_str(
            r#"
#[cfg(any(target_os = "trueos", feature = "trueos-native-testing"))]
mod native_sqlite_backend;
#[cfg(any(target_os = "trueos", feature = "trueos-native-testing"))]
pub fn trueos_initialize() -> Result<(), String> { native_sqlite_backend::initialize() }
"#,
        );
        std::fs::write(path, library).map_err(|e| e.to_string())?;
        let mut backend = include_str!("../apps/sqlite/src/native_backend.rs").to_string();
        // Reuse the production allocator, mutexes, entropy and UTC callbacks.
        replace(
            &mut backend,
            "use rusqlite::ffi;",
            "use crate as ffi;\n#[path = \"native_sqlite_vfs.rs\"]\nmod files;",
        )?;
        replace(
            &mut backend,
            "szOsFile: size_of::<ffi::sqlite3_file>() as c_int,",
            "szOsFile: files::file_size(),",
        )?;
        replace(
            &mut backend,
            "zName: c\"trueos-memory-image\".as_ptr(),",
            "zName: c\"trueos-native-files\".as_ptr(),",
        )?;
        for (old, new) in [
            ("xOpen: Some(vfs_open)", "xOpen: Some(files::open)"),
            ("xDelete: Some(vfs_delete)", "xDelete: Some(files::delete)"),
            ("xAccess: Some(vfs_access)", "xAccess: Some(files::access)"),
            (
                "xFullPathname: Some(vfs_full_pathname)",
                "xFullPathname: Some(files::full_path)",
            ),
        ] {
            replace(&mut backend, old, new)?;
        }
        std::fs::write(staged.join("src/native_sqlite_backend.rs"), backend)
            .map_err(|e| e.to_string())?;
        std::fs::write(
            staged.join("src/native_sqlite_vfs.rs"),
            include_str!("native_sqlite_vfs.rs"),
        )
        .map_err(|e| e.to_string())?;
    } else {
        let path = staged.join("src/inner_connection.rs");
        let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        replace(
            &mut text,
            "        ensure_safe_sqlite_threading_mode()?;",
            r#"
        #[cfg(target_os = "trueos")]
        crate::ffi::trueos_initialize().map_err(|message| Error::SqliteFailure(ffi::Error::new(ffi::SQLITE_ERROR), Some(message)))?;
        ensure_safe_sqlite_threading_mode()?;"#,
        )?;
        std::fs::write(path, text).map_err(|e| e.to_string())?;
    }
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sqlite_stages_exact_registry_boundaries() {
        let requested = std::env::var_os("TRUEOS_NATIVE_SQLITE_PROOF_STAGE");
        let work = requested.as_ref().map(PathBuf::from).unwrap_or_else(|| {
            std::env::temp_dir().join(format!("native-sqlite-staging-{}", std::process::id()))
        });
        for (name, version) in [("libsqlite3-sys", "0.30.1"), ("rusqlite", "0.32.1")] {
            let original = crate::find_cargo_registry_crate(&format!("{name}-{version}")).unwrap();
            stage(&work, &original, name, version).unwrap();
        }
        if requested.is_some() {
            println!("native SQLite proof stage: {}", work.display());
        } else {
            std::fs::remove_dir_all(work).unwrap();
        }
    }
}
