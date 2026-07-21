use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{CargoProfile, PackageCatalog};
use crate::{io_string, parse_string_array, push_feature, toml_string_value};

pub(crate) struct ExampleSpec {
    pub(crate) name: String,
    pub(crate) required_features: Vec<String>,
}

pub(crate) struct PackageAppSpec {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
    pub(crate) manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct AppRegistry {
    apps: Vec<AppRegistryEntry>,
}

#[derive(Deserialize)]
struct ProbeRegistry {
    probes: Vec<AppRegistryEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AppRegistryEntry {
    Name(String),
    Spec {
        name: String,
        path: Option<PathBuf>,
        #[serde(default)]
        optional: bool,
    },
}

struct RegisteredAppSpec {
    name: String,
    dir: PathBuf,
    optional: bool,
}

pub(crate) fn package_name(manifest_path: &Path) -> Result<String, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("name") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err(format!(
        "failed to read package name from {}",
        manifest_path.display()
    ))
}

pub(crate) fn package_bin_name(manifest_path: &Path) -> Result<Option<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_package = false;
    let mut in_bin = false;
    let mut default_run = None;
    let mut first_bin = None;

    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            in_bin = trimmed == "[[bin]]";
            continue;
        }

        if in_package && trimmed.starts_with("default-run") {
            if let Some((_, value)) = trimmed.split_once('=') {
                default_run = toml_string_value(value.trim());
            }
            continue;
        }

        if in_bin && first_bin.is_none() && trimmed.starts_with("name") {
            if let Some((_, value)) = trimmed.split_once('=') {
                first_bin = toml_string_value(value.trim());
            }
        }
    }

    Ok(default_run.or(first_bin))
}

pub(crate) fn package_app_specs(
    app_dir: &Path,
    package_catalog: PackageCatalog,
) -> Result<Vec<PackageAppSpec>, String> {
    let mut specs = Vec::new();
    for app in registered_app_specs(app_dir, package_catalog)? {
        if app.optional && !app.dir.join("Cargo.toml").is_file() {
            println!(
                "trueos-blueprint: skipping unavailable optional {}: {} ({})",
                package_catalog.item_label(),
                app.name,
                app.dir.display()
            );
            continue;
        }
        specs.push(package_app_spec_required(&app)?);
    }
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

pub(crate) fn package_app_spec(
    app_dir: &Path,
    app_name: &str,
    package_catalog: PackageCatalog,
) -> Result<Option<PackageAppSpec>, String> {
    let Some(app) = registered_app_specs(app_dir, package_catalog)?
        .into_iter()
        .find(|app| app.name == app_name)
    else {
        return Ok(None);
    };
    package_app_spec_required(&app).map(Some)
}

fn package_app_spec_required(app: &RegisteredAppSpec) -> Result<PackageAppSpec, String> {
    let app_name = app.name.as_str();
    let dir = app.dir.clone();
    let mut manifest_path = dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "registered app `{app_name}` is missing {}",
            manifest_path.display()
        ));
    }

    let mut name = match package_name(&manifest_path) {
        Ok(name) => name,
        Err(_) => {
            let package_manifest_path = virtual_package_app_manifest_path(&dir, app_name);
            if !package_manifest_path.is_file() {
                return Err(format!(
                    "registered app `{app_name}` has virtual manifest {} without a known package manifest",
                    manifest_path.display()
                ));
            }
            manifest_path = package_manifest_path;
            package_name(&manifest_path)?
        }
    };
    if name != app_name && !virtual_package_app_alias(app_name) {
        return Err(format!(
            "registered app `{app_name}` has package name `{name}` in {}",
            manifest_path.display()
        ));
    }
    if virtual_package_app_alias(app_name) {
        name = app_name.to_string();
    }

    Ok(PackageAppSpec {
        name,
        dir,
        manifest_path,
    })
}

fn virtual_package_app_manifest_path(dir: &Path, app_name: &str) -> PathBuf {
    match app_name {
        "helix" => dir.join("helix-term").join("Cargo.toml"),
        "matrix" => dir.join("src").join("main").join("Cargo.toml"),
        _ => dir.join("src").join("main").join("Cargo.toml"),
    }
}

fn virtual_package_app_alias(app_name: &str) -> bool {
    matches!(
        app_name,
        "fd" | "helix" | "matrix" | "scope_tui" | "aud_player_scope_tui"
    )
}

fn registered_app_specs(
    app_dir: &Path,
    package_catalog: PackageCatalog,
) -> Result<Vec<RegisteredAppSpec>, String> {
    let registry_path = app_dir.join(package_catalog.registry_file());
    let raw = fs::read_to_string(&registry_path)
        .map_err(|err| format!("failed to read {}: {err}", registry_path.display()))?;
    let entries = match package_catalog {
        PackageCatalog::Apps => {
            serde_json::from_str::<AppRegistry>(&raw).map(|registry| registry.apps)
        }
        PackageCatalog::Probes => {
            serde_json::from_str::<ProbeRegistry>(&raw).map(|registry| registry.probes)
        }
    }
    .map_err(|err| format!("failed to parse {}: {err}", registry_path.display()))?;

    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let (name, path, optional) = match entry {
            AppRegistryEntry::Name(name) => (name, None, false),
            AppRegistryEntry::Spec {
                name,
                path,
                optional,
            } => (name, path, optional),
        };
        if name.trim().is_empty() {
            return Err(format!("empty app name in {}", registry_path.display()));
        }
        if out
            .iter()
            .any(|existing: &RegisteredAppSpec| existing.name == name)
        {
            return Err(format!(
                "duplicate registered app `{name}` in {}",
                registry_path.display()
            ));
        }
        let dir = match path {
            Some(path) if path.is_absolute() => path,
            Some(path) => app_dir.join(path),
            None => app_dir
                .join(package_catalog.default_dir())
                .join(name.as_str()),
        };
        out.push(RegisteredAppSpec {
            name,
            dir,
            optional,
        });
    }
    Ok(out)
}

pub(crate) fn package_blueprint_profile(
    manifest_path: &Path,
) -> Result<Option<CargoProfile>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_metadata = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_metadata = trimmed == "[package.metadata.trueos-blueprint]";
            continue;
        }
        if !in_metadata {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "profile" {
            continue;
        }
        return match toml_string_value(value.trim()).as_deref() {
            Some("dev") | Some("debug") => Ok(Some(CargoProfile::Dev)),
            Some("release") => Ok(Some(CargoProfile::Release)),
            Some(other) => Err(format!(
                "unsupported trueos-blueprint profile `{other}` in {}",
                manifest_path.display()
            )),
            None => Err(format!(
                "bad trueos-blueprint profile in {}",
                manifest_path.display()
            )),
        };
    }
    Ok(None)
}

pub(crate) fn package_blueprint_replicatable(manifest_path: &Path) -> Result<bool, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_metadata = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_metadata = trimmed == "[package.metadata.trueos-blueprint]";
            continue;
        }
        if !in_metadata {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "replicatable" {
            continue;
        }
        return match value.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!(
                "bad trueos-blueprint replicatable value in {}",
                manifest_path.display()
            )),
        };
    }
    Ok(false)
}

pub(crate) fn manifest_declared_features(manifest_path: &Path) -> Result<Vec<String>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_features = false;
    let mut out = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features || trimmed.is_empty() {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        out.push(key.trim().to_string());
    }
    Ok(out)
}

pub(crate) fn manifest_has_dependency(
    manifest_path: &Path,
    dependency_name: &str,
) -> Result<bool, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut in_dependencies = false;
    for line in cargo_toml.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed.starts_with('[') {
            let section = trimmed.trim_start_matches('[').trim_end_matches(']');
            in_dependencies = matches!(
                section,
                "dependencies" | "dev-dependencies" | "build-dependencies"
            ) || (section.starts_with("target.")
                && matches!(
                    section.rsplit('.').next(),
                    Some("dependencies" | "dev-dependencies" | "build-dependencies")
                ));
            continue;
        }
        if !in_dependencies || trimmed.is_empty() {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() == dependency_name || key.trim().starts_with(&format!("{dependency_name}.")) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn push_app_or_trueos_feature(
    features: &mut Vec<String>,
    feature: &str,
    declared_features: &[String],
    has_trueos_dependency: bool,
) {
    if declared_features.iter().any(|declared| declared == feature) {
        push_feature(features, feature);
    } else if has_trueos_dependency {
        push_feature(features, &format!("trueos/{feature}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn manifest_has_dependency_sees_target_dependencies() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("trueos-blueprint-manifest-{nonce}.toml"));
        fs::write(
            &path,
            r#"
[package]
name = "probe"
version = "0.1.0"

[target.'cfg(any(target_os = "trueos", target_os = "zkvm"))'.dependencies]
trueos = { path = "../../api" }
"#,
        )
        .unwrap();

        assert!(manifest_has_dependency(&path, "trueos").unwrap());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_replicatable_blueprint_metadata() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("trueos-blueprint-capability-{nonce}.toml"));
        fs::write(
            &path,
            r#"
[package]
name = "probe"
version = "0.1.0"

[package.metadata.trueos-blueprint]
replicatable = true
"#,
        )
        .unwrap();

        assert!(package_blueprint_replicatable(&path).unwrap());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn root_catalogs_keep_apps_and_probes_separate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let apps = registered_app_specs(root, PackageCatalog::Apps).unwrap();
        let probes = package_app_specs(root, PackageCatalog::Probes).unwrap();

        for probe_name in [
            "condvar",
            "cross",
            "framework_stack",
            "panick",
            "posix_fd_probe",
            "rusqlite_probe",
            "rusqlite_multirt",
            "test_some_crates",
            "tokio_fs",
            "tokio_mrt",
            "tokio_net",
            "tokio_rt",
            "unix_api_probe",
            "wls",
        ] {
            assert!(apps.iter().all(|app| app.name != probe_name));
            let probe = probes
                .iter()
                .find(|probe| probe.name == probe_name)
                .unwrap();
            assert_eq!(probe.dir, root.join("probes").join(probe_name));
        }
    }
}

pub(crate) fn example_required_features(
    manifest_path: &Path,
    example_name: &str,
) -> Result<Vec<String>, String> {
    example_specs(manifest_path)?
        .into_iter()
        .find(|example| example.name == example_name)
        .map(|example| example.required_features)
        .ok_or_else(|| format!("unknown example `{example_name}`"))
}

pub(crate) fn example_specs(manifest_path: &Path) -> Result<Vec<ExampleSpec>, String> {
    let cargo_toml = fs::read_to_string(manifest_path).map_err(io_string)?;
    let mut specs = Vec::new();
    let mut in_example = false;
    let mut current_name: Option<String> = None;
    let mut current_required_features = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_example && let Some(name) = current_name.take() {
                specs.push(ExampleSpec {
                    name,
                    required_features: core::mem::take(&mut current_required_features),
                });
            }
            in_example = trimmed == "[[example]]";
            if in_example {
                current_name = None;
                current_required_features.clear();
            }
            continue;
        }
        if !in_example {
            continue;
        }
        if trimmed.starts_with("name") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            current_name = Some(value.trim().trim_matches('"').to_string());
        } else if trimmed.starts_with("required-features") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            current_required_features = parse_string_array(value.trim());
        }
    }
    if in_example && let Some(name) = current_name {
        specs.push(ExampleSpec {
            name,
            required_features: current_required_features,
        });
    }
    Ok(specs)
}
