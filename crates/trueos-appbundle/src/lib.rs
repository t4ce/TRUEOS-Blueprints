//! Shared AppDB bundle metadata, independent of transport and installation.
#![no_std]
extern crate alloc;

use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use serde::Deserialize;

pub const METADATA_KEY: &str = "appbundle.json";
pub const VFILE: &[u8] = b"vFile:appbundle.json";
pub const MAX_MEMBERS: usize = 64;
mod ram_install;
pub use ram_install::{RAM_INSTALL_VFILE, MAX_INSTALL_BYTES, append_ram_install, decode_ram_install};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppBundles {
    version: u32,
    bundles: BTreeMap<String, Bundle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    requires: Vec<String>,
}

pub fn valid_archive(name: &str) -> bool {
    name.strip_suffix(".bp").is_some_and(|stem| {
        !stem.is_empty()
            && stem.len() <= 128
            && stem
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    })
}

impl AppBundles {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let metadata: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid appbundle.json: {error}"))?;
        if metadata.version != 1 {
            return Err(format!(
                "Unsupported appbundle.json version {}",
                metadata.version
            ));
        }
        if metadata.bundles.len() > MAX_MEMBERS {
            return Err("Too many app bundles".into());
        }
        let mut names = Vec::<String>::new();
        for (root, bundle) in &metadata.bundles {
            if !valid_archive(root) || names.iter().any(|name| name.eq_ignore_ascii_case(root)) {
                return Err(format!("Invalid or duplicate app bundle: {root}"));
            }
            names.push(root.clone());
            if bundle.requires.len() > MAX_MEMBERS {
                return Err(format!("Too many requirements for {root}"));
            }
            for (index, member) in bundle.requires.iter().enumerate() {
                if !valid_archive(member)
                    || bundle.requires[..index]
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(member))
                {
                    return Err(format!(
                        "Invalid or duplicate requirement for {root}: {member}"
                    ));
                }
            }
        }
        for root in metadata.bundles.keys() {
            metadata.members(root)?;
        }
        Ok(metadata)
    }

    /// Dependency-first, deduplicated order, ending with the requested app.
    /// An app without a bundle resolves to itself; requirements are one-way.
    pub fn members(&self, archive: &str) -> Result<Vec<String>, String> {
        let mut ordered = Vec::new();
        self.visit(archive, &mut Vec::new(), &mut ordered)?;
        Ok(ordered)
    }

    fn visit(
        &self,
        archive: &str,
        visiting: &mut Vec<String>,
        ordered: &mut Vec<String>,
    ) -> Result<(), String> {
        if ordered
            .iter()
            .any(|name| name.eq_ignore_ascii_case(archive))
        {
            return Ok(());
        }
        if visiting
            .iter()
            .any(|name| name.eq_ignore_ascii_case(archive))
        {
            return Err(format!("App bundle dependency cycle at {archive}"));
        }
        if visiting.len() + ordered.len() >= MAX_MEMBERS {
            return Err("App bundle exceeds 64 Blueprints".into());
        }
        visiting.push(archive.into());
        if let Some((_, bundle)) = self
            .bundles
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(archive))
        {
            for member in &bundle.requires {
                self.visit(member, visiting, ordered)?;
            }
        }
        visiting.pop();
        ordered.push(archive.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_installs_sqlite_and_xerj_service_with_its_engine() {
        let bundles = AppBundles::parse(include_bytes!("../../../appbundle.json")).unwrap();
        assert_eq!(
            bundles.members("codex.bp").unwrap(),
            ["sqlite.bp", "xerj.bp", "xerj-service.bp", "codex.bp"]
        );
        assert_eq!(
            bundles.members("CODEX.bp").unwrap(),
            ["sqlite.bp", "xerj.bp", "xerj-service.bp", "CODEX.bp"]
        );
        assert_eq!(bundles.members("sqlite.bp").unwrap(), ["sqlite.bp"]);
        assert_eq!(bundles.members("xerj.bp").unwrap(), ["xerj.bp"]);
        assert_eq!(
            bundles.members("xerj-service.bp").unwrap(),
            ["xerj.bp", "xerj-service.bp"]
        );
        assert_eq!(bundles.members("Frog.bp").unwrap(), ["Frog.bp"]);
    }

    #[test]
    fn transitive_requirements_are_ordered_and_shared_members_fetched_once() {
        let bundles = AppBundles::parse(
            br#"{"version":1,"bundles":{
            "a.bp":{"requires":["b.bp","c.bp"]},
            "b.bp":{"requires":["c.bp"]}}}"#,
        )
        .unwrap();
        assert_eq!(bundles.members("a.bp").unwrap(), ["c.bp", "b.bp", "a.bp"]);
    }

    #[test]
    fn rejects_cycles_bad_names_versions_and_unknown_fields() {
        for bytes in [
            r#"{"version":1,"bundles":{"a.bp":{"requires":["a.bp"]}}}"#,
            r#"{"version":1,"bundles":{"a.bp":{"requires":["b.bp"]},"b.bp":{"requires":["a.bp"]}}}"#,
            r#"{"version":1,"bundles":{"a.bp":{"requires":["../b.bp"]}}}"#,
            r#"{"version":1,"bundles":{"a.bp":{"requires":["b.bp","B.bp"]}}}"#,
            r#"{"version":2,"bundles":{}}"#,
            r#"{"version":1,"bundles":{},"typo":true}"#,
        ] {
            assert!(AppBundles::parse(bytes.as_bytes()).is_err(), "{bytes}");
        }
    }
}
