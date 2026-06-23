use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const APPS_PUBLISH_SKIP_ENV: &str = "TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH";
const APPS_PUBLISH_MOUNT_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_MOUNT_URI";
const APPS_PUBLISH_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_URI";
const DEFAULT_APPS_PUBLISH_MOUNT_URI: &str = "smb://t4ce@pdjb/home-share";
const DEFAULT_APPS_PUBLISH_URI: &str = "smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps";

pub(crate) fn publish_dist_blueprints(dist_dir: &Path) -> Result<(), String> {
    if env_flag_is_set(APPS_PUBLISH_SKIP_ENV) {
        println!("trueos-blueprint: skipping apps publish");
        return Ok(());
    }

    let target_uri =
        env_string(APPS_PUBLISH_URI_ENV).unwrap_or_else(|| DEFAULT_APPS_PUBLISH_URI.to_string());
    let mount_uri = env_string(APPS_PUBLISH_MOUNT_URI_ENV)
        .unwrap_or_else(|| DEFAULT_APPS_PUBLISH_MOUNT_URI.to_string());
    let bp_files = dist_blueprint_files(dist_dir)?;
    if bp_files.is_empty() {
        return Err(format!("no .bp files found in {}", dist_dir.display()));
    }

    println!("trueos-blueprint: publishing {} blueprints", bp_files.len());
    println!("trueos-blueprint: remote apps dir: {target_uri}");
    let mut mount = gio_command();
    mount.arg("mount").arg(&mount_uri);
    let _ = mount.status();

    ensure_remote_dir(&target_uri);
    clean_remote_dir(&target_uri)?;
    ensure_remote_dir(&target_uri);

    for bp_file in bp_files {
        let mut copy = gio_command();
        copy.arg("copy").arg(&bp_file).arg(&target_uri);
        crate::run_command(&mut copy, "gio copy blueprint")?;
    }

    println!("trueos-blueprint: published dist blueprints");
    Ok(())
}

pub(crate) fn publish_blueprint_file(bp_file: &Path) -> Result<(), String> {
    if env_flag_is_set(APPS_PUBLISH_SKIP_ENV) {
        println!("trueos-blueprint: skipping apps publish");
        return Ok(());
    }
    if !bp_file.is_file() || bp_file.extension().and_then(|value| value.to_str()) != Some("bp") {
        return Err(format!("not a .bp file: {}", bp_file.display()));
    }

    let target_uri =
        env_string(APPS_PUBLISH_URI_ENV).unwrap_or_else(|| DEFAULT_APPS_PUBLISH_URI.to_string());
    let mount_uri = env_string(APPS_PUBLISH_MOUNT_URI_ENV)
        .unwrap_or_else(|| DEFAULT_APPS_PUBLISH_MOUNT_URI.to_string());
    let file_name = bp_file
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("bad blueprint file name: {}", bp_file.display()))?;
    let target_file_uri = join_uri(&target_uri, file_name);

    println!("trueos-blueprint: publishing {}", bp_file.display());
    println!("trueos-blueprint: remote apps dir: {target_uri}");
    let mut mount = gio_command();
    mount.arg("mount").arg(&mount_uri);
    let _ = mount.status();

    ensure_remote_dir(&target_uri);
    let mut remove = gio_command();
    remove.arg("remove").arg("-f").arg(&target_file_uri);
    let _ = remove.status();

    let mut copy = gio_command();
    copy.arg("copy").arg(&bp_file).arg(&target_uri);
    crate::run_command(&mut copy, "gio copy blueprint")?;

    println!("trueos-blueprint: published {}", file_name);
    Ok(())
}

fn dist_blueprint_files(dist_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dist_dir).map_err(io_string)? {
        let entry = entry.map_err(io_string)?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("bp") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn join_uri(base: &str, child: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn clean_remote_dir(uri: &str) -> Result<(), String> {
    for child_uri in gio_list_uris(uri)? {
        clean_remote_entry(&child_uri)?;
    }
    Ok(())
}

fn clean_remote_entry(uri: &str) -> Result<(), String> {
    if let Ok(child_uris) = gio_list_uris(uri) {
        for child_uri in child_uris {
            clean_remote_entry(&child_uri)?;
        }
    }

    let mut remove = gio_command();
    remove.arg("remove").arg("-f").arg(uri);
    crate::run_command(&mut remove, "gio remove remote app entry")
}

fn gio_list_uris(uri: &str) -> Result<Vec<String>, String> {
    let output = gio_command()
        .arg("list")
        .arg("-h")
        .arg("-u")
        .arg(uri)
        .output()
        .map_err(|err| format!("gio list failed to start: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gio list failed for {uri}: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn ensure_remote_dir(uri: &str) {
    let _ = gio_command().arg("mkdir").arg(uri).output();
}

fn gio_command() -> Command {
    let mut cmd = Command::new("gio");
    cmd.env_remove("GIO_MODULE_DIR");
    cmd
}

fn env_flag_is_set(name: &str) -> bool {
    env_string(name).is_some_and(|value| !matches!(value.as_str(), "0" | "false" | "FALSE"))
}

fn env_string(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    if value.is_empty() { None } else { Some(value) }
}

fn io_string(err: io::Error) -> String {
    err.to_string()
}
