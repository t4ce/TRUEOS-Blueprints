//! Keep the named Odin package on its upstream freestanding build path.
use std::{
    env, fs,
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
    command.arg(&build_script).current_dir(&source);
    crate::run_command(&mut command, "upstream Odin native Blueprint build")?;
    let dist = blueprints_root.join("dist");
    fs::create_dir_all(&dist).map_err(|error| format!("create {}: {error}", dist.display()))?;
    let destination = dist.join("odin-son.bp");
    fs::copy(source.join("trueos/build/odin-son.bp"), &destination)
        .map_err(|error| format!("copy native Odin Blueprint: {error}"))?;
    println!("trueos-blueprint: built native {}", destination.display());
    Ok(destination)
}
