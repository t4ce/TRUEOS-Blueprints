//! Dependency-selected, pinned native C allocator and parser providers.
use std::path::Path;

pub(crate) fn patches(
    app: &Path,
    work: &Path,
    selected: &[crate::CratePatch],
) -> Result<Vec<crate::CratePatch>, String> {
    let packages = crate::lock_packages(&crate::app_lockfile(app))?;
    let providers = crate::codex_native_c_overlay::PROVIDERS
        .iter()
        .chain(crate::codex_native_c_overlay::ADDITIONAL_PROVIDERS);
    let mut patches = Vec::new();
    for package in &packages {
        if !package
            .source
            .as_deref()
            .is_some_and(|source| source.starts_with("registry+"))
            || selected.iter().any(|patch| patch.name == package.name)
            || !providers.clone().any(|(name, _)| *name == package.name)
        {
            continue;
        }
        if !providers
            .clone()
            .any(|(name, version)| *name == package.name && *version == package.version)
        {
            return Err(format!(
                "native C dependency requires an audited exact version of {} (found {})",
                package.name, package.version
            ));
        }
        if patches
            .iter()
            .any(|patch: &crate::CratePatch| patch.name == package.name)
        {
            return Err(format!(
                "multiple native C dependency versions for {}",
                package.name
            ));
        }
        let source =
            crate::find_cargo_registry_crate(&format!("{}-{}", package.name, package.version))
                .ok_or_else(|| {
                    format!(
                        "missing original native C source {}-{}",
                        package.name, package.version
                    )
                })?;
        let staged =
            crate::codex_native_c_overlay::stage(work, &source, &package.name, &package.version)?;
        patches.push(crate::CratePatch::new(&package.name, staged));
    }
    Ok(patches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_every_exact_original_native_c_source() {
        let work = std::env::temp_dir().join(format!("native-c-providers-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        for &(name, version) in crate::codex_native_c_overlay::PROVIDERS
            .iter()
            .chain(crate::codex_native_c_overlay::ADDITIONAL_PROVIDERS)
        {
            let source = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
                .expect("cached original C provider");
            let staged =
                crate::codex_native_c_overlay::stage(&work, &source, name, version).unwrap();
            assert!(staged.join("Cargo.toml").is_file());
        }
        std::fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn native_c_selection_preserves_custom_sources_and_rejects_version_drift() {
        let work =
            std::env::temp_dir().join(format!("native-c-source-guards-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(
            work.join("Cargo.toml"),
            "[package]\nname=\"guards\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let registry = "source=\"registry+https://github.com/rust-lang/crates.io-index\"\n";
        std::fs::write(
            work.join("Cargo.lock"),
            format!(
                "version=4\n[[package]]\nname=\"tree-sitter\"\nversion=\"0.26.13\"\n{registry}"
            ),
        )
        .unwrap();
        assert!(
            patches(&work, &work, &[])
                .err()
                .unwrap()
                .contains("audited exact version")
        );
        let custom = [crate::CratePatch::new("tree-sitter", work.join("custom"))];
        assert!(patches(&work, &work, &custom).unwrap().is_empty());
        std::fs::write(
            work.join("Cargo.lock"),
            "version=4\n[[package]]\nname=\"tree-sitter\"\nversion=\"0.26.13\"\n",
        )
        .unwrap();
        assert!(patches(&work, &work, &[]).unwrap().is_empty());
        std::fs::remove_dir_all(work).unwrap();
    }
}
