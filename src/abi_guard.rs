use crate::artifact::tool_command;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const KERNEL_REPO_ENV: &str = "TRUEOS_REPO_ROOT";
const ABI_GUARD_ENV: &str = "TRUEOS_BLUEPRINT_ABI_GUARD";
const ABI_DECLARATIONS_RELATIVE: &str = "crates/trueos-v/src/bp_abi.rs";
const ABI_LOCK_RELATIVE: &str = "abi/portal-cabi-v2.sha256";

#[derive(Debug, Clone, Eq, PartialEq)]
struct FunctionContract {
    canonical: String,
    sha256: String,
}

pub(crate) fn verify_before_pack(
    blueprint_root: Option<&Path>,
    linked_object: &Path,
) -> Result<(), String> {
    if guard_disabled() {
        println!("trueos-blueprint: WARNING: CABI pack guard disabled by {ABI_GUARD_ENV}");
        return Ok(());
    }

    let blueprint_root = blueprint_root.ok_or_else(|| {
        format!(
            "CABI pack guard cannot locate the Blueprint SDK root; run from TRUEOS-Blueprints or set {ABI_GUARD_ENV}=off for an intentional isolated build"
        )
    })?;
    let blueprint_abi = blueprint_root.join(ABI_DECLARATIONS_RELATIVE);
    let kernel_root = locate_kernel_repo(blueprint_root)?;
    let kernel_abi = kernel_root.join(ABI_DECLARATIONS_RELATIVE);

    let imports = undefined_cabi_imports(linked_object)?;
    if imports.is_empty() {
        println!("trueos-blueprint: CABI pack guard: no trueos_cabi imports");
        return Ok(());
    }

    let blueprint_contracts = parse_contract_file(&blueprint_abi)?;
    let kernel_contracts = parse_contract_file(&kernel_abi)?;
    let kernel_contract_digest = aggregate_all_digest(&kernel_contracts);
    verify_kernel_contract_lock(&kernel_root, &kernel_contract_digest)?;
    let mut failures = Vec::new();

    for symbol in &imports {
        match (
            blueprint_contracts.get(symbol),
            kernel_contracts.get(symbol),
        ) {
            (Some(blueprint), Some(kernel)) if blueprint.canonical == kernel.canonical => {}
            (Some(blueprint), Some(kernel)) => failures.push(format!(
                "  {symbol}\n    Blueprint SDK: {} sha256={}\n    kernel SDK:    {} sha256={}",
                blueprint.canonical, blueprint.sha256, kernel.canonical, kernel.sha256,
            )),
            (Some(blueprint), None) => failures.push(format!(
                "  {symbol}\n    Blueprint SDK: {} sha256={}\n    kernel SDK:    MISSING",
                blueprint.canonical, blueprint.sha256,
            )),
            (None, Some(kernel)) => failures.push(format!(
                "  {symbol}\n    Blueprint SDK: MISSING\n    kernel SDK:    {} sha256={}",
                kernel.canonical, kernel.sha256,
            )),
            (None, None) => failures.push(format!(
                "  {symbol}\n    Blueprint SDK: MISSING\n    kernel SDK:    MISSING",
            )),
        }
    }

    if !failures.is_empty() {
        return Err(format!(
            "CABI contract mismatch before pack; no .bp was produced:\n{}\nBlueprint declarations: {}\nkernel declarations: {}\nExisting CABI symbol signatures are immutable. Restore the old signature and add a new versioned/instance symbol instead. Set {KERNEL_REPO_ENV} to compare against another kernel checkout. {ABI_GUARD_ENV}=off is available only for an intentional isolated build.",
            failures.join("\n"),
            blueprint_abi.display(),
            kernel_abi.display(),
        ));
    }

    let blueprint_digest = aggregate_digest(&imports, &blueprint_contracts);
    let kernel_digest = aggregate_digest(&imports, &kernel_contracts);
    println!(
        "trueos-blueprint: CABI pack guard: imports={} blueprint_sha256={} kernel_sha256={} kernel_contract_sha256={} compatible=1",
        imports.len(),
        blueprint_digest,
        kernel_digest,
        kernel_contract_digest,
    );
    Ok(())
}

fn verify_kernel_contract_lock(kernel_root: &Path, actual: &str) -> Result<(), String> {
    let lock_path = kernel_root.join(ABI_LOCK_RELATIVE);
    let expected = fs::read_to_string(&lock_path).map_err(|err| {
        format!(
            "CABI pack guard cannot read kernel contract lock {}: {err}; current normalized kernel contract sha256={actual}",
            lock_path.display(),
        )
    })?;
    let expected = expected
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| format!("CABI contract lock {} is empty", lock_path.display()))?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "CABI contract lock {} must contain one SHA-256 value; found `{expected}`",
            lock_path.display(),
        ));
    }
    if !expected.eq_ignore_ascii_case(actual) {
        return Err(format!(
            "kernel CABI contract changed before pack; no .bp was produced:\n  locked_sha256={expected}\n  actual_sha256={actual}\n  declarations={}\n  lock={}\nExisting symbol signatures are immutable. If the change is strictly additive, review it and update the lock deliberately; otherwise restore the old symbol and add a versioned symbol.",
            kernel_root.join(ABI_DECLARATIONS_RELATIVE).display(),
            lock_path.display(),
        ));
    }
    Ok(())
}

fn guard_disabled() -> bool {
    env::var(ABI_GUARD_ENV)
        .ok()
        .is_some_and(|value| matches!(value.trim(), "0" | "false" | "off" | "no"))
}

fn locate_kernel_repo(blueprint_root: &Path) -> Result<PathBuf, String> {
    let configured = env::var_os(KERNEL_REPO_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let sibling = blueprint_root.parent().map(|parent| parent.join("TRUEOS"));
    for candidate in configured.into_iter().chain(sibling) {
        if candidate.join(ABI_DECLARATIONS_RELATIVE).is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "CABI pack guard cannot find the kernel ABI declarations; set {KERNEL_REPO_ENV} to a TRUEOS checkout containing {ABI_DECLARATIONS_RELATIVE}, or set {ABI_GUARD_ENV}=off for an intentional isolated build"
    ))
}

fn undefined_cabi_imports(object: &Path) -> Result<BTreeSet<String>, String> {
    let mut readelf = tool_command(&["llvm-readelf", "readelf"])?;
    let output = readelf
        .arg("-Ws")
        .arg(object)
        .output()
        .map_err(|err| format!("CABI pack guard failed to start readelf: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "CABI pack guard readelf failed for {} with status {}",
            object.display(),
            output.status,
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("CABI pack guard readelf output was not UTF-8: {err}"))?;
    let mut imports = BTreeSet::new();
    for line in stdout.lines() {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 8 || columns[6] != "UND" {
            continue;
        }
        let symbol = columns
            .last()
            .copied()
            .unwrap_or_default()
            .split('@')
            .next()
            .unwrap_or_default();
        if symbol.starts_with("trueos_cabi_") {
            imports.insert(symbol.to_string());
        }
    }
    Ok(imports)
}

fn parse_contract_file(path: &Path) -> Result<BTreeMap<String, FunctionContract>, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read CABI declarations {}: {err}", path.display()))?;
    parse_contracts(&source).map_err(|err| format!("{}: {err}", path.display()))
}

fn parse_contracts(source: &str) -> Result<BTreeMap<String, FunctionContract>, String> {
    const PREFIX: &str = "pub fn trueos_cabi_";
    let mut contracts = BTreeMap::new();
    let mut cursor = 0usize;
    while let Some(relative_start) = source[cursor..].find(PREFIX) {
        let start = cursor + relative_start;
        let name_start = start + "pub fn ".len();
        let open = source[name_start..]
            .find('(')
            .map(|offset| name_start + offset)
            .ok_or("CABI declaration is missing an argument list")?;
        let name = source[name_start..open].trim();
        if !name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
        {
            return Err(format!("invalid CABI symbol name `{name}`"));
        }
        let close = matching_paren(source, open)
            .ok_or_else(|| format!("unclosed argument list for {name}"))?;
        let semicolon = source[close + 1..]
            .find(';')
            .map(|offset| close + 1 + offset)
            .ok_or_else(|| format!("missing semicolon after {name}"))?;

        let mut argument_types = Vec::new();
        for argument in split_top_level(&source[open + 1..close], ',') {
            let argument = argument.trim();
            if argument.is_empty() {
                continue;
            }
            let argument_type = top_level_colon(argument)
                .map(|colon| &argument[colon + 1..])
                .unwrap_or(argument);
            argument_types.push(normalize_tokens(argument_type));
        }
        let return_source = source[close + 1..semicolon].trim();
        let return_type = return_source
            .strip_prefix("->")
            .map(normalize_tokens)
            .unwrap_or_else(|| "()".to_string());
        let canonical = format!("fn {name}({})->{return_type}", argument_types.join(","));
        let contract = FunctionContract {
            sha256: sha256_hex(canonical.as_bytes()),
            canonical,
        };
        if contracts.insert(name.to_string(), contract).is_some() {
            return Err(format!("duplicate CABI declaration for {name}"));
        }
        cursor = semicolon + 1;
    }
    Ok(contracts)
}

fn matching_paren(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(source: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            _ if character == separator && depth == 0 => {
                parts.push(&source[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&source[start..]);
    parts
}

fn top_level_colon(source: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn normalize_tokens(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn aggregate_digest(
    imports: &BTreeSet<String>,
    contracts: &BTreeMap<String, FunctionContract>,
) -> String {
    let mut hasher = Sha256::new();
    for symbol in imports {
        let contract = &contracts[symbol];
        hasher.update((symbol.len() as u64).to_le_bytes());
        hasher.update(symbol.as_bytes());
        hasher.update((contract.canonical.len() as u64).to_le_bytes());
        hasher.update(contract.canonical.as_bytes());
    }
    digest_hex(&hasher.finalize())
}

fn aggregate_all_digest(contracts: &BTreeMap<String, FunctionContract>) -> String {
    let symbols = contracts.keys().cloned().collect::<BTreeSet<_>>();
    aggregate_digest(&symbols, contracts)
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(&Sha256::digest(bytes))
}

fn digest_hex(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing SHA-256 to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_ignore_names_and_whitespace() {
        let left = parse_contracts(
            r#"unsafe extern "C" {
                pub fn trueos_cabi_probe(value: u32, out: *mut u8) -> i32;
            }"#,
        )
        .unwrap();
        let right = parse_contracts(
            r#"unsafe extern "C" {
                pub fn trueos_cabi_probe(other:u32,result:*mut u8)->i32;
            }"#,
        )
        .unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn signature_hash_changes_with_argument_layout() {
        let old = parse_contracts(
            "unsafe extern \"C\" { pub fn trueos_cabi_probe(generation: u64, ptr: *const u8); }",
        )
        .unwrap();
        let changed = parse_contracts(
            "unsafe extern \"C\" { pub fn trueos_cabi_probe(instance: u32, generation: u64, ptr: *const u8); }",
        )
        .unwrap();
        assert_ne!(old["trueos_cabi_probe"], changed["trueos_cabi_probe"]);
    }

    #[test]
    fn nested_function_pointer_arguments_split_correctly() {
        let contracts = parse_contracts(
            r#"unsafe extern "C" {
                pub fn trueos_cabi_probe(callback: unsafe extern "C" fn(u32, *mut u8) -> i32, count: usize);
            }"#,
        )
        .unwrap();
        assert_eq!(
            contracts["trueos_cabi_probe"].canonical,
            "fn trueos_cabi_probe(unsafeextern\"C\"fn(u32,*mutu8)->i32,usize)->()"
        );
    }
}
