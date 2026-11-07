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
const KERNEL_PREBIND_RELATIVE: &str = "src/hv/blueprint/prebind.rs";
const KERNEL_UNIX_COMPAT_RELATIVE: &str = "src/unix_compat.rs";

#[derive(Debug, Clone, Eq, PartialEq)]
struct FunctionContract {
    canonical: String,
    sha256: String,
}

pub(crate) fn verify_before_pack(
    blueprint_root: Option<&Path>,
    linked_object: &Path,
) -> Result<(), String> {
    // Loader compatibility is required even when CABI signature comparison is
    // deliberately disabled or no kernel checkout can be discovered.
    let imports = undefined_imports(linked_object)?;
    verify_launchable_imports(&imports)?;
    let kernel_root = blueprint_root
        .map(locate_kernel_repo)
        .transpose()?
        .flatten();
    if let Some(kernel_root) = &kernel_root {
        verify_kernel_launchable_imports(&imports, kernel_root)?;
    }
    if guard_disabled() {
        println!("trueos-blueprint: WARNING: CABI pack guard disabled by {ABI_GUARD_ENV}");
        return Ok(());
    }

    let Some(blueprint_root) = blueprint_root else {
        println!(
            "trueos-blueprint: CABI pack guard skipped: Blueprint SDK root unavailable and no sibling TRUEOS checkout can be discovered"
        );
        return Ok(());
    };
    let blueprint_abi = blueprint_root.join(ABI_DECLARATIONS_RELATIVE);
    let Some(kernel_root) = kernel_root else {
        println!(
            "trueos-blueprint: CABI pack guard skipped: no sibling directory named trueos (case-insensitive)"
        );
        return Ok(());
    };
    let kernel_abi = kernel_root.join(ABI_DECLARATIONS_RELATIVE);

    let imports = imports
        .into_iter()
        .filter(|symbol| symbol.starts_with("trueos_cabi_"))
        .collect::<BTreeSet<_>>();
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
            "CABI contract mismatch before pack; no .bp was produced:\n{}\nBlueprint declarations: {}\nkernel declarations: {}\nExisting CABI symbol signatures are immutable. Restore the old signature and add a new versioned/instance symbol instead. Set {KERNEL_REPO_ENV} to compare against another kernel checkout. {ABI_GUARD_ENV}=off is available for an intentional migration build.",
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

pub(crate) fn locate_kernel_repo(blueprint_root: &Path) -> Result<Option<PathBuf>, String> {
    if let Some(configured) = env::var_os(KERNEL_REPO_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if configured.join(ABI_DECLARATIONS_RELATIVE).is_file() {
            return Ok(Some(configured));
        }
        return Err(format!(
            "CABI pack guard cannot find kernel ABI declarations at configured {KERNEL_REPO_ENV}={} (expected {ABI_DECLARATIONS_RELATIVE})",
            configured.display(),
        ));
    }

    let Some(parent) = blueprint_root.parent() else {
        return Ok(None);
    };
    let entries = fs::read_dir(parent).map_err(|err| {
        format!(
            "CABI pack guard cannot inspect Blueprint sibling directory {}: {err}",
            parent.display(),
        )
    })?;
    let mut siblings = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|err| {
                format!(
                    "CABI pack guard cannot inspect an entry in Blueprint sibling directory {}: {err}",
                    parent.display(),
                )
            })?
            .path();
        if path.is_dir() && is_trueos_directory_name(&path) {
            siblings.push(path);
        }
    }
    siblings.sort();

    match siblings.as_slice() {
        [] => Ok(None),
        [sibling] if sibling.join(ABI_DECLARATIONS_RELATIVE).is_file() => Ok(Some(sibling.clone())),
        [sibling] => Err(format!(
            "CABI pack guard found sibling TRUEOS directory {} but it does not contain {ABI_DECLARATIONS_RELATIVE}; set {ABI_GUARD_ENV}=off only for an intentional migration build",
            sibling.display(),
        )),
        _ => Err(format!(
            "CABI pack guard found multiple case-insensitive TRUEOS sibling directories ({}); set {KERNEL_REPO_ENV} to select one explicitly",
            siblings
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

fn is_trueos_directory_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("trueos"))
}

fn undefined_imports(object: &Path) -> Result<BTreeSet<String>, String> {
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
    Ok(parse_undefined_imports(&stdout))
}

fn parse_undefined_imports(stdout: &str) -> BTreeSet<String> {
    let mut imports = BTreeSet::new();
    for line in stdout.lines() {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 8 || columns[6] != "UND" {
            continue;
        }
        // GNU readelf may append a version index, e.g. `name@VER (2)`.
        // The name is column eight, not necessarily the final column.
        let symbol = columns[7].split('@').next().unwrap_or_default();
        imports.insert(symbol.to_string());
    }
    imports
}

fn verify_launchable_imports(imports: &BTreeSet<String>) -> Result<(), String> {
    let mut failures = Vec::new();
    for symbol in imports {
        let reason = match symbol.as_str() {
            "pthread_create" | "pthread_join" | "pthread_detach" | "pthread_kill" => Some(
                "POSIX thread lifecycle is unavailable; rebuild with the TRUEOS std backend using trueos_cabi_thread_*",
            ),
            "opendir" | "fdopendir" | "readdir" | "readdir_r" | "closedir" | "dirfd" => {
                Some("POSIX directory streams are unavailable; use trueos::async_fs::list_dir")
            }
            "trueos_tokio_tls_current_slot" => {
                Some("legacy TLS import; rebuild against the WLS SDK")
            }
            "trueos_gl_get_proc_address" => Some(
                "TRUEOS GLES procedure bridge is not implemented; glutin can compile but Alacritty cannot load or render",
            ),
            name if name.starts_with("trueos_cabi_fs_")
                || name.starts_with("trueos_cabi_trueosfs_") =>
            {
                Some("synchronous filesystem ABI removed; use trueos::async_fs")
            }
            _ => None,
        };
        if let Some(reason) = reason {
            failures.push(format!("  {symbol}: {reason}"));
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Blueprint has loader-blocked imports; no new .bp was produced:\n{}\nAny existing dist or published package is from an earlier successful build.",
        failures.join("\n"),
    ))
}

#[derive(Debug, Default)]
struct KernelRetirements {
    exact: BTreeSet<String>,
    prefixes: BTreeSet<String>,
}

impl KernelRetirements {
    fn rejects(&self, symbol: &str) -> bool {
        self.exact.contains(symbol)
            || self
                .prefixes
                .iter()
                .any(|prefix| symbol.starts_with(prefix))
    }
}

fn verify_kernel_launchable_imports(
    imports: &BTreeSet<String>,
    kernel: &Path,
) -> Result<(), String> {
    let read = |relative: &str| {
        fs::read_to_string(kernel.join(relative)).map_err(|error| {
            format!(
                "cannot verify kernel loader retirements in {}: {error}",
                kernel.join(relative).display()
            )
        })
    };
    let rules = parse_kernel_retirements(
        &read(KERNEL_PREBIND_RELATIVE)?,
        &read(KERNEL_UNIX_COMPAT_RELATIVE)?,
    )
    .map_err(|error| {
        format!(
            "cannot verify kernel loader retirement source in {}: {error}",
            kernel.display()
        )
    })?;
    let rejected = imports
        .iter()
        .filter(|symbol| rules.rejects(symbol))
        .collect::<Vec<_>>();
    if !rejected.is_empty() {
        return Err(format!(
            "Blueprint imports APIs retired by the selected kernel; no new .bp was produced:\n{}\nKernel loader sources: {}, {}.\nRebuild against native TRUEOS APIs. Disabling the CABI signature guard does not bypass loader retirement checks.",
            rejected
                .iter()
                .map(|symbol| format!("  {symbol}"))
                .collect::<Vec<_>>()
                .join("\n"),
            kernel.join(KERNEL_PREBIND_RELATIVE).display(),
            kernel.join(KERNEL_UNIX_COMPAT_RELATIVE).display(),
        ));
    }
    println!(
        "trueos-blueprint: kernel loader retirement guard: imports={} compatible=1 kernel={}",
        imports.len(),
        kernel.display()
    );
    Ok(())
}

// Read the loader's maintained rules rather than copying its historical Unix
// classifier into the SDK. The accepted Rust shapes are deliberately narrow:
// an unrecognized edit fails the build instead of silently weakening the gate.
fn parse_kernel_retirements(prebind: &str, unix: &str) -> Result<KernelRetirements, String> {
    let retired = source_function(prebind, "retired_import_error")?;
    let mut rules = KernelRetirements {
        exact: match_literal_names(&retired, false)?,
        prefixes: BTreeSet::new(),
    };
    // Only unconditional prefix rejection branches may precede the literal
    // match. New guards/operators must be reviewed instead of being ignored.
    let mut at = 0;
    while retired.get(at).is_some_and(|token| token.word("if")) {
        let branch = retired
            .get(at..at + 8)
            .ok_or("incomplete retired prefix branch")?;
        let prefix = match branch {
            [
                keyword,
                name,
                SourceToken::Punct('.'),
                method,
                SourceToken::Punct('('),
                SourceToken::Literal(prefix),
                SourceToken::Punct(')'),
                SourceToken::Punct('{'),
            ] if keyword.word("if") && name.word("name") && method.word("starts_with") => prefix,
            _ => return Err("unrecognized retired prefix condition".into()),
        };
        validate_symbol(prefix)?;
        let close = matching_source_delimiter(&retired, at + 7)?;
        let body = &retired[at + 8..close];
        if !matches!(body,
            [keyword, some, SourceToken::Punct('('), SourceToken::Literal(_), SourceToken::Punct(')'), SourceToken::Punct(';')]
            | [keyword, some, SourceToken::Punct('('), SourceToken::Literal(_), SourceToken::Punct(','), SourceToken::Punct(')'), SourceToken::Punct(';')]
            if keyword.word("return") && some.word("Some"))
        {
            return Err("unrecognized retired prefix result".into());
        }
        rules.prefixes.insert(prefix.clone());
        at = close + 1;
    }
    if !retired.get(at..at + 3).is_some_and(|tokens| {
        tokens[0].word("match") && tokens[1].word("name") && tokens[2] == SourceToken::Punct('{')
    }) {
        return Err("unrecognized retired loader source before literal match".into());
    }
    let predicate = source_function(unix, "unsupported_unix_import_reason")?;
    if predicate == [SourceToken::Word("None".into())] {
        return Ok(rules);
    }
    let condition_start = predicate.iter().position(|token| token.word("if")).ok_or(
        "unsupported_unix_import_reason must contain the recognized retirement predicate or None",
    )? + 1;
    let condition_end = predicate[condition_start..]
        .iter()
        .position(|token| *token == SourceToken::Punct('{'))
        .map(|offset| condition_start + offset)
        .ok_or("unclosed Unix retirement predicate")?;
    let expected = source_tokens("is_unix_import(name) && resolve_import(name).is_none()")?;
    if predicate[condition_start..condition_end] != expected {
        return Err(
            "unrecognized Unix retirement predicate; review the kernel loader guard parser".into(),
        );
    }
    let predicate_close = matching_source_delimiter(&predicate, condition_end)?;
    if !predicate
        .get(condition_end + 1)
        .is_some_and(|token| token.word("Some"))
        || predicate[predicate_close + 1..] != source_tokens("else { None }")?
    {
        return Err("unrecognized Unix retirement result; expected Some(reason) else None".into());
    }
    let classified = source_function(unix, "is_unix_import")?;
    let macro_start = classified
        .windows(5)
        .position(|window| {
            window[0].word("matches")
                && window[1] == SourceToken::Punct('!')
                && window[2] == SourceToken::Punct('(')
                && window[3].word("name")
                && window[4] == SourceToken::Punct(',')
        })
        .ok_or("is_unix_import must use matches!(name, literal patterns)")?;
    let close = matching_source_delimiter(&classified, macro_start + 2)?;
    let historical = literal_patterns(&classified[macro_start + 5..close])?;
    let providers = match_literal_names(&source_function(unix, "resolve_import")?, true)?;
    rules
        .exact
        .extend(historical.difference(&providers).cloned());
    Ok(rules)
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SourceToken {
    Word(String),
    Literal(String),
    Punct(char),
}

impl SourceToken {
    fn word(&self, value: &str) -> bool {
        matches!(self, Self::Word(word) if word == value)
    }
}

fn source_tokens(source: &str) -> Result<Vec<SourceToken>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if bytes[at..].starts_with(b"//") {
            at += 2;
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes[at..].starts_with(b"/*") {
            at += 2;
            let mut depth = 1;
            while at < bytes.len() && depth != 0 {
                if bytes[at..].starts_with(b"/*") {
                    depth += 1;
                    at += 2;
                } else if bytes[at..].starts_with(b"*/") {
                    depth -= 1;
                    at += 2;
                } else {
                    at += 1;
                }
            }
            if depth != 0 {
                return Err("unclosed source comment".into());
            }
            continue;
        }
        if bytes[at] == b'"' {
            at += 1;
            let mut value = String::new();
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' {
                    at += 1;
                    if at == bytes.len() {
                        return Err("unclosed source string escape".into());
                    }
                    // Escaped diagnostic strings are irrelevant to symbol patterns.
                    value.push('\\');
                }
                value.push(bytes[at] as char);
                at += 1;
            }
            if at == bytes.len() {
                return Err("unclosed source string".into());
            }
            at += 1;
            tokens.push(SourceToken::Literal(value));
            continue;
        }
        if bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            tokens.push(SourceToken::Word(source[start..at].into()));
        } else {
            tokens.push(SourceToken::Punct(bytes[at] as char));
            at += 1;
        }
    }
    Ok(tokens)
}

fn matching_source_delimiter(tokens: &[SourceToken], open: usize) -> Result<usize, String> {
    let SourceToken::Punct(start) = tokens[open] else {
        return Err("expected source delimiter".into());
    };
    let end = match start {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        _ => return Err("invalid source delimiter".into()),
    };
    let mut stack = vec![end];
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        if let SourceToken::Punct(symbol) = token {
            match symbol {
                '(' => stack.push(')'),
                '{' => stack.push('}'),
                '[' => stack.push(']'),
                ')' | '}' | ']' => {
                    if stack.pop() != Some(*symbol) {
                        return Err("mismatched source delimiter".into());
                    }
                    if stack.is_empty() {
                        return Ok(index);
                    }
                }
                _ => {}
            }
        }
    }
    Err("unclosed source delimiter".into())
}

fn source_function(source: &str, name: &str) -> Result<Vec<SourceToken>, String> {
    let tokens = source_tokens(source)?;
    let start = tokens
        .windows(2)
        .position(|window| window[0].word("fn") && window[1].word(name))
        .ok_or_else(|| format!("missing kernel loader function {name}"))?;
    let open = tokens
        .iter()
        .enumerate()
        .skip(start + 2)
        .find(|(_, token)| **token == SourceToken::Punct('{'))
        .map(|(index, _)| index)
        .ok_or_else(|| format!("missing body for {name}"))?;
    let close = matching_source_delimiter(&tokens, open)?;
    Ok(tokens[open + 1..close].to_vec())
}

fn validate_symbol(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("unrecognized loader symbol literal {value:?}"));
    }
    Ok(())
}

fn literal_patterns(patterns: &[SourceToken]) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut expect_literal = true;
    for token in patterns {
        match token {
            SourceToken::Literal(name) if expect_literal => {
                validate_symbol(name)?;
                names.insert(name.clone());
                expect_literal = false;
            }
            SourceToken::Punct('|') if !expect_literal => expect_literal = true,
            SourceToken::Punct(',') if !expect_literal => {}
            _ => return Err("loader patterns must be literal names separated by |".into()),
        }
    }
    if expect_literal || names.is_empty() {
        return Err("empty or incomplete loader literal pattern".into());
    }
    Ok(names)
}

fn match_literal_names(
    tokens: &[SourceToken],
    providers_only: bool,
) -> Result<BTreeSet<String>, String> {
    let start = tokens
        .windows(3)
        .position(|window| {
            window[0].word("match")
                && window[1].word("name")
                && window[2] == SourceToken::Punct('{')
        })
        .ok_or("kernel loader function must contain match name")?
        + 2;
    let close = matching_source_delimiter(tokens, start)?;
    let mut names = BTreeSet::new();
    let mut at = start + 1;
    while at < close {
        if tokens[at] == SourceToken::Punct(',') {
            at += 1;
            continue;
        }
        let arrow = tokens[at..close]
            .windows(2)
            .position(|window| window == [SourceToken::Punct('='), SourceToken::Punct('>')])
            .map(|offset| at + offset)
            .ok_or("missing loader match arm arrow")?;
        let wildcard = tokens[at..arrow] == [SourceToken::Word("_".into())];
        let rhs = arrow + 2;
        let has_provider = tokens.get(rhs).is_some_and(|token| token.word("Some"));
        if wildcard && !tokens.get(rhs).is_some_and(|token| token.word("None")) {
            return Err("kernel loader wildcard must resolve to None".into());
        }
        if !wildcard && (!providers_only || has_provider) {
            names.extend(literal_patterns(&tokens[at..arrow])?);
        }
        if !wildcard && !has_provider {
            return Err("unrecognized kernel resolver arm; expected Some(provider)".into());
        }
        at = rhs;
        while at < close && tokens[at] != SourceToken::Punct(',') {
            if matches!(tokens[at], SourceToken::Punct('(' | '{' | '[')) {
                at = matching_source_delimiter(tokens, at)? + 1;
            } else {
                at += 1;
            }
        }
    }
    Ok(names)
}

fn parse_contract_file(path: &Path) -> Result<BTreeMap<String, FunctionContract>, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read CABI declarations {}: {err}", path.display()))?;
    parse_contracts(&source).map_err(|err| format!("{}: {err}", path.display()))
}

fn parse_contracts(source: &str) -> Result<BTreeMap<String, FunctionContract>, String> {
    const PREFIXES: &[&str] = &[
        "pub fn trueos_cabi_",
        "pub safe fn trueos_cabi_",
        "pub unsafe fn trueos_cabi_",
    ];
    let mut contracts = BTreeMap::new();
    let mut cursor = 0usize;
    while cursor < source.len() {
        let Some((start, prefix)) = PREFIXES
            .iter()
            .filter_map(|prefix| {
                source[cursor..]
                    .find(prefix)
                    .map(|offset| (cursor + offset, *prefix))
            })
            .min_by_key(|(start, _)| *start)
        else {
            break;
        };
        let name_start = start + prefix.len() - "trueos_cabi_".len();
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

    const PREBIND_FIXTURE: &str = r#"
        // "comment_must_not_be_retired" => Some("ignored")
        fn retired_import_error(name: &str) -> Option<&'static str> {
            if name.starts_with("retired_crypto_") { return Some("old crypto"); }
            match name {
                "exit" | "trueos_cabi_alloc" => Some("retired allocation/process"),
                "old_native" => Some("retired native symbol"),
                _ => None,
            }
        }
    "#;
    const UNIX_FIXTURE: &str = r#"
        fn unsupported_unix_import_reason(name: &str) -> Option<&'static str> {
            if is_unix_import(name) && resolve_import(name).is_none() {
                Some("unsupported Unix API")
            } else { None }
        }
        fn is_unix_import(name: &str) -> bool {
            matches!(name, "read" | "pthread_mutex_lock" | "memchr" | "log")
        }
        fn resolve_import(name: &str) -> Option<usize> {
            match name {
                "memchr" => Some(crate::helpers::memchr as *const () as usize),
                "log" => Some(crate::math::log as *const () as usize),
                _ => None,
            }
        }
    "#;

    #[test]
    fn kernel_retirement_rules_follow_names_prefixes_and_actual_providers() {
        let rules = parse_kernel_retirements(PREBIND_FIXTURE, UNIX_FIXTURE).unwrap();
        for name in [
            "exit",
            "trueos_cabi_alloc",
            "old_native",
            "read",
            "pthread_mutex_lock",
            "retired_crypto_sha",
        ] {
            assert!(rules.rejects(name), "accepted {name}");
        }
        for name in [
            "memchr",
            "log",
            "trueos_cabi_thread_spawn",
            "comment_must_not_be_retired",
            "retired_crypto",
        ] {
            assert!(!rules.rejects(name), "rejected {name}");
        }
    }

    #[test]
    fn inactive_kernel_retirement_does_not_retire_the_historical_classifier() {
        let unix = UNIX_FIXTURE.replace(
            "if is_unix_import(name) && resolve_import(name).is_none() {\n                Some(\"unsupported Unix API\")\n            } else { None }",
            "None",
        );
        let rules = parse_kernel_retirements(PREBIND_FIXTURE, &unix).unwrap();
        assert!(!rules.rejects("read"));
        assert!(rules.rejects("exit"));
    }

    #[test]
    fn kernel_retirement_source_drift_fails_closed() {
        for unix in [
            UNIX_FIXTURE.replace("&& resolve_import(name).is_none()", ""),
            UNIX_FIXTURE.replace("Some(\"unsupported Unix API\")", "None"),
            UNIX_FIXTURE.replace("\"memchr\" => Some", "\"memchr\" => custom_provider"),
            UNIX_FIXTURE.replace("matches!(name,", "custom_classifier(name,"),
        ] {
            assert!(parse_kernel_retirements(PREBIND_FIXTURE, &unix).is_err());
        }
        assert!(parse_kernel_retirements("fn unrelated() {}", UNIX_FIXTURE).is_err());
        assert!(
            parse_kernel_retirements(
                &PREBIND_FIXTURE.replace("starts_with", "ends_with"),
                UNIX_FIXTURE
            )
            .is_err()
        );
        assert!(
            parse_kernel_retirements(
                &PREBIND_FIXTURE.replace("_ => None", "_ => Some(\"all\")"),
                UNIX_FIXTURE
            )
            .is_err()
        );
    }

    #[test]
    fn source_lexer_ignores_nested_comments_and_quoted_fake_functions() {
        let prebind = format!(
            "/* nested /* fn retired_import_error() {{ }} */ */ const TEXT: &str = \"fn retired_import_error() {{ fake }}\"; {PREBIND_FIXTURE}"
        );
        let rules = parse_kernel_retirements(&prebind, UNIX_FIXTURE).unwrap();
        assert!(rules.rejects("old_native"));
        assert!(!rules.rejects("fake"));
    }

    #[test]
    fn current_kernel_retirements_reject_old_sqlite_but_allow_native_helpers() {
        let blueprint = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(kernel) = locate_kernel_repo(blueprint).unwrap() else {
            return;
        };
        let imports = BTreeSet::from([
            "read".into(),
            "open64".into(),
            "getpid".into(),
            "exit".into(),
            "pthread_mutex_lock".into(),
            "trueos_cabi_alloc".into(),
            "ring_core_probe".into(),
        ]);
        let error = verify_kernel_launchable_imports(&imports, &kernel).unwrap_err();
        for symbol in imports {
            assert!(error.contains(&symbol), "{symbol}: {error}");
        }
        verify_kernel_launchable_imports(
            &BTreeSet::from([
                "memchr".into(),
                "log".into(),
                "abs".into(),
                "qsort".into(),
                "trueos_time_unix_nanos".into(),
                "trueos_cabi_thread_spawn".into(),
            ]),
            &kernel,
        )
        .unwrap();
    }

    #[test]
    fn kernel_loader_gate_precedes_the_signature_guard_override() {
        // Verify the real entry point's ordering without mutating process env
        // while other SDK tests can be running in parallel.
        let entry = source_function(include_str!("abi_guard.rs"), "verify_before_pack").unwrap();
        let loader = entry
            .iter()
            .position(|token| token.word("verify_kernel_launchable_imports"))
            .unwrap();
        let disabled = entry
            .iter()
            .position(|token| token.word("guard_disabled"))
            .unwrap();
        assert!(loader < disabled);
    }

    #[test]
    fn versioned_undefined_symbols_keep_the_name_before_the_version_index() {
        let imports = parse_undefined_imports(
            "  1: 0000000000000000 0 FUNC GLOBAL DEFAULT UND pthread_detach@GLIBC_2.34 (2)\n\
               2: 0000000000000100 0 FUNC GLOBAL DEFAULT 1 pthread_join\n\
               3: 0000000000000000 0 NOTYPE GLOBAL DEFAULT UND trueos_cabi_poll_once\n",
        );
        assert_eq!(
            imports,
            BTreeSet::from(["pthread_detach".into(), "trueos_cabi_poll_once".into()])
        );
        assert!(
            verify_launchable_imports(&imports)
                .unwrap_err()
                .contains("pthread_detach")
        );
    }

    #[test]
    fn loader_gate_rejects_all_removed_surfaces_together() {
        let imports = BTreeSet::from([
            "pthread_kill".into(),
            "trueos_cabi_fs_read_file".into(),
            "opendir".into(),
            "trueos_tokio_tls_current_slot".into(),
        ]);
        let error = verify_launchable_imports(&imports).unwrap_err();
        for symbol in imports {
            assert!(error.contains(&symbol));
        }
        assert!(
            verify_launchable_imports(&BTreeSet::from([
                "pthread_mutex_lock".into(),
                "trueos_cabi_async_fs_read_start".into(),
                "trueos_service_lane_submit_job".into(),
                "trueos_cabi_thread_spawn".into(),
                "trueos_cabi_thread_join".into(),
                "trueos_cabi_thread_detach".into(),
                "raise".into(),
            ]))
            .is_ok()
        );
    }

    #[test]
    fn loader_gate_rejects_the_unimplemented_gles_bridge() {
        let error =
            verify_launchable_imports(&BTreeSet::from(["trueos_gl_get_proc_address".into()]))
                .unwrap_err();
        assert!(error.contains("Alacritty cannot load or render"));
    }

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
    fn safe_and_unsafe_import_qualifiers_keep_the_same_contract() {
        let safe = parse_contracts(
            r#"unsafe extern "C" {
                pub safe fn trueos_cabi_probe(value: u32) -> i32;
                pub unsafe fn trueos_cabi_unsafe_probe(value: *const u8) -> i32;
            }"#,
        )
        .unwrap();
        let ordinary = parse_contracts(
            r#"unsafe extern "C" {
                pub fn trueos_cabi_probe(other: u32) -> i32;
                pub fn trueos_cabi_unsafe_probe(other: *const u8) -> i32;
            }"#,
        )
        .unwrap();
        assert_eq!(safe["trueos_cabi_probe"], ordinary["trueos_cabi_probe"]);
        assert_eq!(
            safe["trueos_cabi_unsafe_probe"],
            ordinary["trueos_cabi_unsafe_probe"]
        );
    }

    #[test]
    fn safe_import_signature_hash_changes_with_argument_type() {
        let old = parse_contracts(
            "unsafe extern \"C\" { pub safe fn trueos_cabi_probe(value: u64) -> i32; }",
        )
        .unwrap();
        let changed = parse_contracts(
            "unsafe extern \"C\" { pub safe fn trueos_cabi_probe(value: u32) -> i32; }",
        )
        .unwrap();
        assert_ne!(old["trueos_cabi_probe"], changed["trueos_cabi_probe"]);
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

    #[test]
    fn trueos_sibling_name_is_ascii_case_insensitive() {
        for name in ["trueos", "TRUEOS", "TrueOS", "tRuEoS"] {
            assert!(is_trueos_directory_name(Path::new(name)));
        }
        assert!(!is_trueos_directory_name(Path::new("TRUEOS-Blueprints")));
        assert!(!is_trueos_directory_name(Path::new("trueos-kernel")));
    }
}
