//! Verify that a packed Codex retains the TRUEOS workload placement hooks.
//!
//! A string in the image does not prove that code calls the kernel. Require
//! strong undefined ELF symbols, and reject local definitions of those hooks.

use crate::artifact::tool_command;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const REQUIRED_IMPORTS: &[&str] = &[
    "trueos_cabi_thread_configure_placement_v1",
    "trueos_cabi_thread_swap_activity_v1",
    "trueos_cabi_thread_swap_child_role_v1",
];

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(crate) struct Verification {
    pub(crate) elf_sha256: String,
    pub(crate) required_imports: Vec<String>,
}

impl Verification {
    /// Check persisted evidence before trusting a hash-bound build receipt.
    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_digest(&self.elf_sha256)?;
        if self.required_imports != REQUIRED_IMPORTS {
            return Err("Codex workload receipt has an incomplete or invalid import set".into());
        }
        Ok(())
    }
}

pub(crate) fn verify(elf: &Path) -> Result<Verification, String> {
    let output = tool_command(&["llvm-readelf", "readelf"])?
        .args(["-W", "--symbols"])
        .arg(elf)
        .output()
        .map_err(|error| {
            format!(
                "cannot inspect Codex workload ELF {}: {error}",
                elf.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect Codex workload ELF {}: {}",
            elf.display(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let symbols = String::from_utf8(output.stdout)
        .map_err(|error| format!("invalid Codex workload ELF symbol output: {error}"))?;
    let required_imports = verify_symbols(&symbols)?;
    Ok(Verification {
        elf_sha256: sha256(elf)?,
        required_imports,
    })
}

fn verify_symbols(symbols: &str) -> Result<Vec<String>, String> {
    let mut undefined = BTreeSet::new();
    let mut defined = BTreeSet::new();
    for line in symbols.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        // readelf -W --symbols columns: Num Value Size Type Bind Vis Ndx Name.
        // Validate the numeric prefix so headings and incidental strings cannot
        // be mistaken for entries in the symbol table.
        if fields.len() < 8
            || fields[0]
                .strip_suffix(':')
                .and_then(|value| value.parse::<u64>().ok())
                .is_none()
            || !REQUIRED_IMPORTS.contains(&fields[7])
        {
            continue;
        }
        let name = fields[7];
        if fields[6] != "UND" {
            defined.insert(name);
        } else if fields[4] == "GLOBAL" {
            undefined.insert(name);
        }
    }
    if !defined.is_empty() {
        return Err(format!(
            "Codex workload hooks must be undefined kernel imports, but the ELF defines: {}",
            defined.into_iter().collect::<Vec<_>>().join(", "),
        ));
    }
    let missing: Vec<_> = REQUIRED_IMPORTS
        .iter()
        .copied()
        .filter(|name| !undefined.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Codex workload ELF is missing required kernel imports: {}",
            missing.join(", "),
        ));
    }
    Ok(REQUIRED_IMPORTS.iter().map(|name| (*name).into()).collect())
}

pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Verify the actual container on cache hits without expanding its ELF again.
pub(crate) fn verify_blueprint_digest(
    blueprint: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    validate_digest(expected_sha256)?;
    if sha256(blueprint)? != expected_sha256 {
        return Err(format!(
            "Codex workload receipt does not match Blueprint {}",
            blueprint.display(),
        ));
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Codex workload receipt contains an invalid SHA-256 digest".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn symbols_without(omitted: Option<&str>) -> String {
        REQUIRED_IMPORTS
            .iter()
            .filter(|name| Some(**name) != omitted)
            .enumerate()
            .map(|(index, name)| {
                format!(" {index}: 0000000000000000 0 NOTYPE GLOBAL DEFAULT UND {name}\n")
            })
            .collect()
    }

    #[test]
    fn requires_each_kernel_import() {
        assert_eq!(
            verify_symbols(&symbols_without(None)).unwrap(),
            REQUIRED_IMPORTS,
        );
        for omitted in REQUIRED_IMPORTS {
            let error = verify_symbols(&symbols_without(Some(omitted))).unwrap_err();
            assert!(error.contains(omitted), "{error}");
        }
    }

    #[test]
    fn rejects_defined_stubs_even_if_an_undefined_entry_is_present() {
        for hook in REQUIRED_IMPORTS {
            let symbols = format!(
                "{} 100: 0000000000001000 1 FUNC GLOBAL DEFAULT 1 {hook}\n",
                symbols_without(None),
            );
            let error = verify_symbols(&symbols).unwrap_err();
            assert!(error.contains("ELF defines"), "{error}");
            assert!(error.contains(hook), "{error}");
        }
    }

    #[test]
    fn strings_and_weak_references_do_not_satisfy_imports() {
        let hook = REQUIRED_IMPORTS[0];
        let strings = format!(
            "{} incidental 00000000 0 NOTYPE GLOBAL DEFAULT UND {hook}\n",
            symbols_without(Some(hook)),
        );
        assert!(verify_symbols(&strings).is_err());
        let weak = format!(
            "{} 100: 00000000 0 NOTYPE WEAK DEFAULT UND {hook}\n",
            symbols_without(Some(hook)),
        );
        assert!(verify_symbols(&weak).is_err());
    }

    #[test]
    fn verifies_real_elf_and_rejects_a_linked_stub() {
        let directory = std::env::temp_dir().join(format!(
            "codex-workload-elf-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let source = directory.join("probe.c");
        let object = directory.join("probe.o");
        let declarations: String = REQUIRED_IMPORTS
            .iter()
            .map(|hook| format!("extern void {hook}(void);\n"))
            .collect();
        let calls: String = REQUIRED_IMPORTS
            .iter()
            .map(|hook| format!("{hook}();\n"))
            .collect();
        let compile = || {
            let output = tool_command(&["clang", "cc"])
                .unwrap()
                .arg("-c")
                .arg(&source)
                .arg("-o")
                .arg(&object)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr),
            );
        };
        fs::write(
            &source,
            format!("{declarations}\nvoid probe(void) {{ {calls} }}\n"),
        )
        .unwrap();
        compile();
        let evidence = verify(&object).unwrap();
        evidence.validate().unwrap();
        assert_eq!(evidence.elf_sha256, sha256(&object).unwrap());

        fs::write(
            &source,
            format!(
                "{declarations}\nvoid {}(void) {{}}\nvoid probe(void) {{ {calls} }}\n",
                REQUIRED_IMPORTS[0],
            ),
        )
        .unwrap();
        compile();
        let error = verify(&object).unwrap_err();
        assert!(error.contains("ELF defines"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn validates_receipt_evidence_after_deserialization() {
        let good = Verification {
            elf_sha256: "a".repeat(64),
            required_imports: REQUIRED_IMPORTS.iter().map(|name| (*name).into()).collect(),
        };
        let json = serde_json::to_vec(&good).unwrap();
        let restored: Verification = serde_json::from_slice(&json).unwrap();
        restored.validate().unwrap();
        for index in 0..REQUIRED_IMPORTS.len() {
            let mut incomplete = restored.clone();
            incomplete.required_imports.remove(index);
            assert!(incomplete.validate().is_err());
        }
        let mut duplicate = restored.clone();
        duplicate.required_imports.push(REQUIRED_IMPORTS[0].into());
        assert!(duplicate.validate().is_err());
        let mut invalid_digest = restored;
        invalid_digest.elf_sha256 = "g".repeat(64);
        assert!(invalid_digest.validate().is_err());
    }

    #[test]
    fn receipt_hash_rejects_a_changed_blueprint() {
        let path = std::env::temp_dir().join(format!(
            "codex-workload-digest-test-{}-{}.bp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        fs::write(&path, b"original Blueprint").unwrap();
        let digest = sha256(&path).unwrap();
        verify_blueprint_digest(&path, &digest).unwrap();
        fs::write(&path, b"modified Blueprint").unwrap();
        assert!(verify_blueprint_digest(&path, &digest).is_err());
        assert!(verify_blueprint_digest(&path, "bad digest").is_err());
        fs::remove_file(path).unwrap();
    }
}
