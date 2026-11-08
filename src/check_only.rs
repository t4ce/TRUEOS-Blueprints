//! Compile the ordinary staged target without linking, packing or publishing.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};
pub(crate) fn take_flag(args: &mut Vec<OsString>) -> Result<bool, String> {
    let count = args.iter().filter(|arg| *arg == "--check").count();
    if count > 1 {
        return Err("--check may be supplied only once".into());
    }
    args.retain(|arg| arg != "--check");
    Ok(count == 1)
}
pub(crate) fn command(configured: &Command) -> Result<Command, String> {
    let mut check = Command::new(configured.get_program());
    if let Some(dir) = configured.get_current_dir() {
        check.current_dir(dir);
    }
    for (key, value) in configured.get_envs() {
        if let Some(value) = value {
            check.env(key, value);
        } else {
            check.env_remove(key);
        }
    }
    let mut changed = false;
    for arg in configured.get_args() {
        if arg == "--" {
            return Err("check-only command must be selected before rustc-only arguments".into());
        }
        if !changed && arg == "rustc" {
            check.arg("check");
            changed = true;
        } else {
            check.arg(arg);
        }
    }
    if !changed {
        return Err("check-only requires the configured cargo rustc command".into());
    }
    Ok(check)
}
pub(crate) fn receipt(path: &Path, command: &Command) -> Result<PathBuf, String> {
    let value = serde_json::json!({"schema_version":1,"status":"checked-no-package","package_produced":false,"command":std::iter::once(command.get_program()).chain(command.get_args()).map(|v|v.to_string_lossy()).collect::<Vec<_>>(),"cwd":command.get_current_dir()});
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "trueos-blueprint: check passed; no package was linked, packed or published: {}",
        path.display()
    );
    Ok(path.to_path_buf())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_flag_is_removed_without_changing_app_selection() {
        let mut args = vec!["codex".into(), "--check".into(), "--release".into()];
        assert!(take_flag(&mut args).unwrap());
        assert_eq!(
            args,
            vec![OsString::from("codex"), OsString::from("--release")]
        );
        assert!(!take_flag(&mut args).unwrap());
        assert!(take_flag(&mut vec!["--check".into(), "--check".into()]).is_err());
    }
    #[test]
    fn check_preserves_native_staging_configuration_but_rejects_codegen_arguments() {
        let mut normal = Command::new("cargo");
        normal
            .current_dir("/native/staged")
            .env("RUSTC", "/native/private-rustc")
            .env_remove("REMOVED")
            .args([
                "+pinned",
                "rustc",
                "--target",
                "native.json",
                "--features",
                "native-net",
                "--release",
                "--config",
                "patch.source=staged",
                "--message-format=json-render-diagnostics",
            ]);
        let check = command(&normal).unwrap();
        let expected = normal
            .get_args()
            .map(|v| {
                if v == "rustc" {
                    OsString::from("check")
                } else {
                    v.to_os_string()
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            check.get_args().collect::<Vec<_>>(),
            expected.iter().map(OsString::as_os_str).collect::<Vec<_>>()
        );
        assert_eq!(check.get_current_dir(), normal.get_current_dir());
        assert_eq!(
            check.get_envs().collect::<Vec<_>>(),
            normal.get_envs().collect::<Vec<_>>()
        );
        normal.args(["--", "--emit=obj"]);
        assert!(command(&normal).is_err());
    }
    #[test]
    fn successful_check_receipt_cannot_replace_a_stale_package() {
        let dir = std::env::temp_dir().join(format!("trueos-check-receipt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bp = dir.join("codex.bp");
        std::fs::write(&bp, b"old-package-sentinel").unwrap();
        let path = dir.join("native-check.json");
        let result = receipt(&path, &Command::new("cargo")).unwrap();
        assert_eq!(result, path);
        assert_ne!(result, bp);
        assert_eq!(std::fs::read(&bp).unwrap(), b"old-package-sentinel");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(result).unwrap()).unwrap();
        assert_eq!(value["package_produced"], false);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
