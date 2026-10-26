//! Keep the named Odin package on its upstream freestanding build path.
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn build(blueprints_root: &Path) -> Result<PathBuf, String> {
    let source = env::var_os("TRUEOS_ODIN_SON_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| blueprints_root.join("../odin-son"));
    let source = fs::canonicalize(&source)
        .map_err(|error| format!("resolve native Odin source {}: {error}", source.display()))?;
    let build_script = source.join("trueos/build.sh");
    if !build_script.is_file() {
        return Err(format!(
            "missing native Odin build script: {}",
            build_script.display()
        ));
    }
    let mut command = Command::new("bash");
    let mut roots = Vec::new();
    for key in ["ODIN_SDK_ROOT", "ODIN_ROOT"] {
        if let Some(root) = env::var_os(key) {
            roots.push(PathBuf::from(root));
        }
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.extend([
            home.join(".local/bin"),
            home.join("odin"),
            home.join("Programs/odin"),
            // This install layout is documented in docs/native-odin.md.
            home.join("Programmme/odin"),
        ]);
    }
    let compiler = find_odin(
        env::var_os("ODIN").as_deref(),
        env::var_os("PATH").as_deref(),
        &roots,
    )?;
    println!("trueos-blueprint: Odin compiler: {}", compiler.display());
    command
        .arg(&build_script)
        .current_dir(&source)
        .env("ODIN", compiler);
    crate::run_command(&mut command, "upstream Odin native Blueprint build")?;
    let dist = blueprints_root.join("dist");
    fs::create_dir_all(&dist).map_err(|error| format!("create {}: {error}", dist.display()))?;
    let destination = dist.join("odin-son.bp");
    fs::copy(source.join("trueos/build/odin-son.bp"), &destination)
        .map_err(|error| format!("copy native Odin Blueprint: {error}"))?;
    println!("trueos-blueprint: built native {}", destination.display());
    Ok(destination)
}

fn find_odin(
    explicit: Option<&OsStr>,
    search_path: Option<&OsStr>,
    roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let path_dirs: Vec<_> = search_path
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    let candidates = if let Some(name) = explicit {
        let path = PathBuf::from(name);
        if path.components().count() > 1 || path.is_absolute() {
            vec![path]
        } else {
            path_dirs.iter().map(|dir| dir.join(name)).collect()
        }
    } else {
        path_dirs
            .iter()
            .map(|dir| dir.join("odin"))
            .chain(
                roots
                    .iter()
                    .flat_map(|root| [root.join("odin"), root.join("bin/odin")]),
            )
            .collect()
    };
    for candidate in candidates {
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return fs::canonicalize(&candidate)
            .map_err(|error| format!("resolve Odin compiler {}: {error}", candidate.display()));
    }
    if let Some(name) = explicit {
        Err(format!(
            "ODIN={} does not resolve to an executable compiler",
            name.to_string_lossy()
        ))
    } else {
        Err("Odin compiler not found; set ODIN=/absolute/path/to/odin, add it to PATH, or set ODIN_SDK_ROOT to its installation directory".to_string())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn compiler_discovery_preserves_overrides_and_finds_install_roots() {
        let temp = env::temp_dir().join(format!("trueos-odin-discovery-{}", std::process::id()));
        let path_dir = temp.join("path");
        let install = temp.join("Programmme/odin");
        for dir in [&path_dir, &install] {
            fs::create_dir_all(dir).unwrap();
            let binary = dir.join("odin");
            fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = env::join_paths([&path_dir]).unwrap();
        let roots = [install.clone()];
        assert_eq!(
            find_odin(None, Some(&path), &roots).unwrap(),
            fs::canonicalize(path_dir.join("odin")).unwrap()
        );
        assert_eq!(
            find_odin(None, None, &roots).unwrap(),
            fs::canonicalize(install.join("odin")).unwrap()
        );
        assert_eq!(
            find_odin(Some(install.join("odin").as_os_str()), Some(&path), &roots).unwrap(),
            fs::canonicalize(install.join("odin")).unwrap()
        );
        assert!(find_odin(Some(OsStr::new("missing-odin")), Some(&path), &roots).is_err());
        fs::set_permissions(install.join("odin"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(find_odin(None, None, &roots).is_err());
        fs::remove_dir_all(temp).unwrap();
    }
}
