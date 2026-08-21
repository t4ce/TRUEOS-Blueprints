use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use serde::Deserialize;

use crate::cli::PackageCatalog;

const APPS_PUBLISH_SKIP_ENV: &str = "TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH";
const APPS_PUBLISH_MOUNT_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_MOUNT_URI";
const APPS_PUBLISH_URI_ENV: &str = "TRUEOS_BLUEPRINT_APPS_PUBLISH_URI";
const PROBES_PUBLISH_URI_ENV: &str = "TRUEOS_BLUEPRINT_PROBES_PUBLISH_URI";
const DEFAULT_APPS_PUBLISH_MOUNT_URI: &str = "smb://t4ce@pdjb/home-share";
const DEFAULT_APPS_PUBLISH_URI: &str = "smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps";
const DEFAULT_PROBES_PUBLISH_URI: &str = "smb://t4ce@pdjb/home-share/TRUEOS_SITE/probes";
const PUBLISHED_APP_HASH_SEPARATOR: &str = "§§";

#[derive(Deserialize)]
struct BuildinsManifest {
    buildins: Vec<String>,
}

pub(crate) fn publish_blueprint_files(
    bp_files: &[PathBuf],
    package_catalog: PackageCatalog,
) -> Result<(), String> {
    if env_flag_is_set(APPS_PUBLISH_SKIP_ENV) {
        println!(
            "trueos-blueprint: skipping {} publish",
            package_catalog.default_dir()
        );
        return Ok(());
    }

    if bp_files.is_empty() {
        return Err("no .bp files were built for publishing".to_string());
    }
    let published_files = bp_files
        .iter()
        .filter_map(|path| match is_buildin_app(path, package_catalog) {
            Ok(true) => {
                println!(
                    "trueos-blueprint: skipping build-in app publish: {}",
                    path.display()
                );
                None
            }
            Ok(false) => Some(Ok(path)),
            Err(err) => Some(Err(err)),
        })
        .map(|path| {
            let path = path?;
            let published_name = published_blueprint_name(path)?;
            Ok((path.clone(), published_name))
        })
        .collect::<Result<Vec<_>, String>>()?;

    if published_files.is_empty() {
        println!("trueos-blueprint: no publishable blueprints remain");
        return Ok(());
    }

    let target_uri = publish_uri(package_catalog);
    let mount_uri = env_string(APPS_PUBLISH_MOUNT_URI_ENV)
        .unwrap_or_else(|| DEFAULT_APPS_PUBLISH_MOUNT_URI.to_string());

    println!(
        "trueos-blueprint: publishing {} blueprints",
        published_files.len()
    );
    println!(
        "trueos-blueprint: remote {} dir: {target_uri}",
        package_catalog.default_dir()
    );
    let mut mount = gio_command();
    mount.arg("mount").arg(&mount_uri);
    let _ = mount.status();

    ensure_remote_dir(&target_uri);
    clean_remote_dir(&target_uri)?;
    ensure_remote_dir(&target_uri);

    for (bp_file, published_name) in published_files {
        let target_file_uri = join_uri(&target_uri, published_name.as_str());
        let mut copy = gio_command();
        copy.arg("copy").arg(&bp_file).arg(&target_file_uri);
        crate::run_command(&mut copy, "gio copy blueprint")?;
    }

    println!("trueos-blueprint: published dist blueprints");
    Ok(())
}

pub(crate) fn publish_blueprint_file(
    bp_file: &Path,
    package_catalog: PackageCatalog,
) -> Result<(), String> {
    if env_flag_is_set(APPS_PUBLISH_SKIP_ENV) {
        println!(
            "trueos-blueprint: skipping {} publish",
            package_catalog.default_dir()
        );
        return Ok(());
    }
    if !bp_file.is_file() || bp_file.extension().and_then(|value| value.to_str()) != Some("bp") {
        return Err(format!("not a .bp file: {}", bp_file.display()));
    }
    if is_buildin_app(bp_file, package_catalog)? {
        println!(
            "trueos-blueprint: skipping build-in app publish: {}",
            bp_file.display()
        );
        return Ok(());
    }

    let target_uri = publish_uri(package_catalog);
    let mount_uri = env_string(APPS_PUBLISH_MOUNT_URI_ENV)
        .unwrap_or_else(|| DEFAULT_APPS_PUBLISH_MOUNT_URI.to_string());
    let file_name = bp_file
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("bad blueprint file name: {}", bp_file.display()))?;
    let published_name = published_blueprint_name(bp_file)?;
    let target_file_uri = join_uri(&target_uri, published_name.as_str());

    println!("trueos-blueprint: publishing {}", bp_file.display());
    println!(
        "trueos-blueprint: remote {} dir: {target_uri}",
        package_catalog.default_dir()
    );
    let mut mount = gio_command();
    mount.arg("mount").arg(&mount_uri);
    let _ = mount.status();

    ensure_remote_dir(&target_uri);
    remove_published_blueprint_versions(&target_uri, file_name)?;

    let mut copy = gio_command();
    copy.arg("copy").arg(&bp_file).arg(&target_file_uri);
    crate::run_command(&mut copy, "gio copy blueprint")?;

    println!("trueos-blueprint: published {}", published_name);
    Ok(())
}

fn is_buildin_app(bp_file: &Path, package_catalog: PackageCatalog) -> Result<bool, String> {
    if package_catalog != PackageCatalog::Apps {
        return Ok(false);
    }

    let bp_file = fs::canonicalize(bp_file).map_err(io_string)?;
    let repository_root = bp_file
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("cannot locate repository root for {}", bp_file.display()))?;
    let manifest_path = repository_root.join("buildins.json");
    if !manifest_path.is_file() {
        return Ok(false);
    }

    let manifest: BuildinsManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(io_string)?,
    )
    .map_err(|err| format!("invalid {}: {err}", manifest_path.display()))?;
    let app_name = bp_file
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("bad blueprint file name: {}", bp_file.display()))?;
    Ok(manifest.buildins.iter().any(|name| name == app_name))
}

fn publish_uri(package_catalog: PackageCatalog) -> String {
    let (env_name, default_uri) = publish_uri_config(package_catalog);
    env_string(env_name).unwrap_or_else(|| default_uri.to_string())
}

fn publish_uri_config(package_catalog: PackageCatalog) -> (&'static str, &'static str) {
    match package_catalog {
        PackageCatalog::Apps => (APPS_PUBLISH_URI_ENV, DEFAULT_APPS_PUBLISH_URI),
        PackageCatalog::Probes => (PROBES_PUBLISH_URI_ENV, DEFAULT_PROBES_PUBLISH_URI),
    }
}

fn published_blueprint_name(bp_file: &Path) -> Result<String, String> {
    let file_name = bp_file
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("bad blueprint file name: {}", bp_file.display()))?;
    let bytes = fs::read(bp_file).map_err(io_string)?;
    let digest = Sha256::digest(bytes.as_slice());
    let mut sha256 = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(sha256, "{byte:02x}").expect("writing SHA-256 to String cannot fail");
    }
    Ok(format!("{file_name}{PUBLISHED_APP_HASH_SEPARATOR}{sha256}"))
}

fn remove_published_blueprint_versions(target_uri: &str, file_name: &str) -> Result<(), String> {
    for child_uri in gio_list_uris(target_uri)? {
        let Some(segment) = child_uri.rsplit('/').next() else {
            continue;
        };
        let decoded = percent_decode_uri_segment(segment);
        let is_same_app = decoded == file_name
            || decoded
                .strip_prefix(file_name)
                .is_some_and(|suffix| suffix.starts_with(PUBLISHED_APP_HASH_SEPARATOR));
        if !is_same_app {
            continue;
        }

        let mut remove = gio_command();
        remove.arg("remove").arg("-f").arg(&child_uri);
        crate::run_command(&mut remove, "gio remove old published blueprint")?;
    }
    Ok(())
}

fn percent_decode_uri_segment(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(out.as_slice()).into_owned()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_repository(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "trueos-publish-{}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("dist")).unwrap();
        root
    }

    #[test]
    fn published_name_preserves_single_section_and_uses_double_separator() {
        let path =
            env::temp_dir().join(format!("trueos-publish-{}-one§app.bp", std::process::id()));
        fs::write(&path, b"abc").unwrap();

        let published = published_blueprint_name(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(
            published,
            format!(
                "{}§§{}",
                path.file_name().unwrap().to_string_lossy(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
        );
    }

    #[test]
    fn percent_decode_preserves_the_double_section_delimiter() {
        assert_eq!(
            percent_decode_uri_segment("one%C2%A7app.bp%C2%A7%C2%A7abc123"),
            "one§app.bp§§abc123"
        );
    }

    #[test]
    fn apps_and_probes_have_separate_default_publish_uris() {
        assert_eq!(
            publish_uri_config(PackageCatalog::Apps),
            (APPS_PUBLISH_URI_ENV, DEFAULT_APPS_PUBLISH_URI)
        );
        assert_eq!(
            publish_uri_config(PackageCatalog::Probes),
            (PROBES_PUBLISH_URI_ENV, DEFAULT_PROBES_PUBLISH_URI)
        );
        assert!(DEFAULT_APPS_PUBLISH_URI.ends_with("/TRUEOS_SITE/apps"));
        assert!(DEFAULT_PROBES_PUBLISH_URI.ends_with("/TRUEOS_SITE/probes"));
    }

    #[test]
    fn buildins_manifest_excludes_apps_but_not_probes() {
        let root = temporary_repository("buildins");
        let bp_file = root.join("dist/commander.bp");
        fs::write(&bp_file, b"blueprint").unwrap();
        fs::write(
            root.join("buildins.json"),
            br#"{"buildins":["commander"]}"#,
        )
        .unwrap();

        assert!(is_buildin_app(&bp_file, PackageCatalog::Apps).unwrap());
        assert!(!is_buildin_app(&bp_file, PackageCatalog::Probes).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ordinary_apps_remain_publishable() {
        let root = temporary_repository("ordinary-app");
        let bp_file = root.join("dist/calculator.bp");
        fs::write(&bp_file, b"blueprint").unwrap();
        fs::write(
            root.join("buildins.json"),
            br#"{"buildins":["commander"]}"#,
        )
        .unwrap();

        assert!(!is_buildin_app(&bp_file, PackageCatalog::Apps).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
