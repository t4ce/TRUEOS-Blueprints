//! Resolve named package builds using the same dependency order as Apps installs.

use std::fs;
use std::io;
use std::path::Path;

use crate::app_catalog::{PackageAppSpec, package_app_spec, package_bin_name, package_name};
use crate::cli::PackageCatalog;
use trueos_appbundle::AppBundles;

fn archive_name(package: &PackageAppSpec) -> Result<String, String> {
    let binary =
        package_bin_name(&package.manifest_path)?.unwrap_or(package_name(&package.manifest_path)?);
    Ok(format!("{binary}.bp"))
}

/// Resolve every companion before building any of them. Each returned package
/// goes through the existing single-package build and its ordinary cache.
pub(crate) fn resolve(
    root: &Path,
    selected: PackageAppSpec,
    catalog: PackageCatalog,
) -> Result<Vec<PackageAppSpec>, String> {
    if catalog != PackageCatalog::Apps {
        return Ok(vec![selected]);
    }
    let metadata = root.join(trueos_appbundle::METADATA_KEY);
    let bytes = match fs::read(&metadata) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![selected]),
        Err(error) => return Err(format!("read {}: {error}", metadata.display())),
    };
    let bundles = AppBundles::parse(&bytes)?;
    let selected_archive = archive_name(&selected)?;
    let members = bundles.members(&selected_archive)?;
    let mut selected = Some(selected);
    let mut packages = Vec::with_capacity(members.len());
    for archive in members {
        if archive.eq_ignore_ascii_case(&selected_archive) {
            packages.push(selected.take().expect("selected archive occurs once"));
        } else {
            packages.push(resolve_companion(root, &archive, catalog)?);
        }
    }
    Ok(packages)
}

fn resolve_companion(
    root: &Path,
    archive: &str,
    catalog: PackageCatalog,
) -> Result<PackageAppSpec, String> {
    let stem = archive
        .strip_suffix(".bp")
        .ok_or("invalid bundle archive")?;
    if let Some(package) = package_app_spec(root, stem, catalog)?
        && archive_name(&package)?.eq_ignore_ascii_case(archive)
    {
        return Ok(package);
    }
    // A registry name may differ from its executable (codex-cli -> codex,
    // xerj-server -> xerj). Optional entries remain explicitly selectable;
    // unrelated absent optional checkouts must not block a named bundle.
    let registry = root.join(catalog.registry_file());
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(&registry).map_err(|error| format!("read {}: {error}", registry.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", registry.display()))?;
    let entries = value[catalog.default_dir()]
        .as_array()
        .ok_or_else(|| format!("invalid package registry {}", registry.display()))?;
    for entry in entries {
        let Some(name) = entry.as_str().or_else(|| entry["name"].as_str()) else {
            continue;
        };
        if let Ok(Some(package)) = package_app_spec(root, name, catalog)
            && archive_name(&package)?.eq_ignore_ascii_case(archive)
        {
            return Ok(package);
        }
    }
    Err(format!(
        "bundle companion {archive} has no buildable package in {}",
        registry.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("blueprint-bundle-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn package(&self, name: &str, binary: Option<&str>) {
            let dir = self.0.join("apps").join(name);
            fs::create_dir_all(&dir).unwrap();
            let mut manifest = format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\n");
            if let Some(binary) = binary {
                manifest.push_str(&format!(
                    "[[bin]]\nname = {binary:?}\npath = \"src/main.rs\"\n"
                ));
            }
            fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        }
        fn resolve(&self, selected: &str) -> Result<Vec<PackageAppSpec>, String> {
            let package = package_app_spec(&self.0, selected, PackageCatalog::Apps)?.unwrap();
            resolve(&self.0, package, PackageCatalog::Apps)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn codex_builds_its_runtime_companions_in_install_order_without_xerj() {
        let fixture = Fixture::new();
        for name in ["sqlite", "lily2vec", "ptts", "ttstt"] {
            fixture.package(name, None);
        }
        fixture.package("codex-cli", Some("codex"));
        fs::write(fixture.0.join("apps.json"), r#"{"apps":["codex-cli","sqlite","lily2vec","ptts","ttstt",{"name":"xerj-server","optional":true}]}"#).unwrap();
        fs::write(
            fixture.0.join("appbundle.json"),
            include_bytes!("../appbundle.json"),
        )
        .unwrap();
        let packages = fixture.resolve("codex-cli").unwrap();
        assert_eq!(
            packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>(),
            ["sqlite", "lily2vec", "ptts", "codex-cli"]
        );
        assert_eq!(fixture.resolve("ttstt").unwrap().len(), 1);
    }

    #[test]
    fn transitive_optional_companions_resolve_by_binary_and_are_deduplicated() {
        let fixture = Fixture::new();
        fixture.package("service", None);
        fixture.package("search-server", Some("search"));
        fixture.package("database", None);
        fs::write(
            fixture.0.join("apps.json"),
            r#"{"apps":["service",{"name":"search-server","optional":true},"database"]}"#,
        )
        .unwrap();
        fs::write(fixture.0.join("appbundle.json"), r#"{"version":1,"bundles":{"service.bp":{"requires":["search.bp","database.bp"]},"search.bp":{"requires":["database.bp"]}}}"#).unwrap();
        let packages = fixture.resolve("service").unwrap();
        assert_eq!(
            packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>(),
            ["database", "search-server", "service"]
        );
    }

    #[test]
    fn missing_companion_fails_resolution_before_any_build() {
        let fixture = Fixture::new();
        fixture.package("service", None);
        fs::write(fixture.0.join("apps.json"), r#"{"apps":["service"]}"#).unwrap();
        fs::write(
            fixture.0.join("appbundle.json"),
            r#"{"version":1,"bundles":{"service.bp":{"requires":["missing.bp"]}}}"#,
        )
        .unwrap();
        let error = fixture.resolve("service").err().unwrap();
        assert!(error.contains("missing.bp"), "{error}");
        assert!(!fixture.0.join("dist").exists());
    }
}
