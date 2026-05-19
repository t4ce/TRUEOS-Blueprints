use std::fs;
use std::path::{Path, PathBuf};

use super::{parse_string_array, push_feature};

// Planning stays separate from cargo/link execution so future SDK steps can
// compose around a stable target-analysis boundary.
#[derive(Clone)]
pub(crate) enum BuildTarget {
    Package,
    Example(String),
}

#[derive(Clone, Copy)]
pub(crate) enum BuildFlavor {
    TokioStd,
    ThinNoStd,
}

impl BuildFlavor {
    pub(crate) fn cache_label(self) -> &'static str {
        match self {
            BuildFlavor::TokioStd => "tokio-platform",
            BuildFlavor::ThinNoStd => "thin-nostd",
        }
    }
}

pub(crate) struct BuildSettings {
    pub(crate) flavor: BuildFlavor,
    pub(crate) source_path: PathBuf,
    pub(crate) has_global_allocator: bool,
    pub(crate) has_panic_handler: bool,
    pub(crate) needs_tokio_net: bool,
    pub(crate) needs_no_std_shim: bool,
    pub(crate) needs_entry_shim: bool,
    pub(crate) extra_features: Vec<String>,
}

pub(crate) fn resolve_build_settings(
    app_dir: &Path,
    manifest_path: &Path,
    build_target: &BuildTarget,
) -> Result<BuildSettings, String> {
    let source_path = match build_target {
        BuildTarget::Package => package_source_path(app_dir, manifest_path)?,
        BuildTarget::Example(name) => example_source_path(app_dir, manifest_path, name)?,
    };
    let source = fs::read_to_string(&source_path)
        .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
    let needs_tokio_net = source_needs_tokio_net(&source);
    let needs_trueos_platform = source_needs_trueos_platform(&source);
    let explicit_no_std = source_is_explicit_no_std(&source);
    let flavor = if !explicit_no_std
        || needs_tokio_net
        || needs_trueos_platform
        || source.contains("trueos_blueprint")
        || source.contains("trueos_blueprint::")
        || source.contains("tokio::")
    {
        BuildFlavor::TokioStd
    } else {
        BuildFlavor::ThinNoStd
    };
    let mut extra_features = blueprint_feature_directives(&source);
    if matches!(flavor, BuildFlavor::TokioStd) {
        push_feature(&mut extra_features, "tokio-runtime");
    }
    if needs_tokio_net {
        push_feature(&mut extra_features, "tokio-net-probe");
    }
    Ok(BuildSettings {
        flavor,
        source_path,
        has_global_allocator: source.contains("#[global_allocator]"),
        has_panic_handler: source.contains("#[panic_handler]"),
        needs_tokio_net,
        needs_no_std_shim: matches!(flavor, BuildFlavor::ThinNoStd) && !explicit_no_std,
        needs_entry_shim: source.contains("fn main(") && !source.contains("#![no_main]"),
        extra_features,
    })
}

fn package_source_path(app_dir: &Path, manifest_path: &Path) -> Result<PathBuf, String> {
    if let Some(path) = package_bin_source_path(app_dir, manifest_path)? {
        return Ok(path);
    }

    for candidate in [app_dir.join("src/main.rs"), app_dir.join("src/lib.rs")] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "missing package source; expected src/main.rs or src/lib.rs under {}",
        app_dir.display()
    ))
}

fn package_bin_source_path(
    app_dir: &Path,
    manifest_path: &Path,
) -> Result<Option<PathBuf>, String> {
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("bad manifest path: {}", manifest_path.display()))?;
    let cargo_toml = fs::read_to_string(manifest_path)
        .map_err(|err| format!("failed to read {}: {err}", manifest_path.display()))?;
    let mut in_bin = false;
    let mut current_path: Option<String> = None;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_bin && let Some(path) = current_path.take() {
                return Ok(Some(manifest_dir.join(path)));
            }
            in_bin = trimmed == "[[bin]]";
            current_path = None;
            continue;
        }
        if !in_bin {
            continue;
        }
        if trimmed.starts_with("path")
            && let Some((_, value)) = trimmed.split_once('=')
        {
            current_path = Some(value.trim().trim_matches('"').to_string());
        }
    }

    if in_bin && let Some(path) = current_path {
        return Ok(Some(manifest_dir.join(path)));
    }

    Ok(None)
}

fn source_needs_tokio_net(source: &str) -> bool {
    source.contains("tokio::net")
        || source.contains("trueos::net")
        || source.contains("trueos_blueprint::net")
        || source.contains("current_thread_net")
        || source.contains("net::TcpListener")
        || source.contains("net::TcpStream")
        || source.contains("net::UdpSocket")
        || source.contains("net::mio")
        || source.contains("mio::net")
        || source.contains("socket2::")
}

fn source_needs_trueos_platform(source: &str) -> bool {
    source.contains("trueos::runtime")
        || source.contains("trueos::task")
        || source.contains("trueos::sync")
        || source.contains("trueos::time")
        || source.contains("trueos::io")
        || source.contains("trueos::fs")
        || source_group_import_mentions(
            source,
            "trueos::{",
            &[
                "runtime", "task", "sync", "time", "io", "fs", "net", "tokio",
            ],
        )
}

fn source_group_import_mentions(source: &str, prefix: &str, names: &[&str]) -> bool {
    let mut rest = source;
    while let Some((_, after_prefix)) = rest.split_once(prefix) {
        let Some((group, after_group)) = after_prefix.split_once(';') else {
            return false;
        };
        if names.iter().any(|name| {
            group
                .split(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
                .any(|token| token == *name)
        }) {
            return true;
        }
        rest = after_group;
    }
    false
}

fn source_is_explicit_no_std(source: &str) -> bool {
    source.contains("#![no_std]") || source.contains("#![cfg_attr(not(test), no_std)]")
}

fn blueprint_feature_directives(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let Some((_, suffix)) = line.split_once("trueos-blueprint:") else {
            continue;
        };
        let Some((key, value)) = suffix.split_once('=') else {
            continue;
        };
        if key.trim() != "features" {
            continue;
        }
        for feature in parse_string_array(value.trim()) {
            push_feature(&mut out, &feature);
        }
    }
    out
}

fn example_source_path(
    app_dir: &Path,
    manifest_path: &Path,
    example_name: &str,
) -> Result<PathBuf, String> {
    let cargo_toml = fs::read_to_string(manifest_path)
        .map_err(|err| format!("failed to read {}: {err}", manifest_path.display()))?;
    let mut in_example = false;
    let mut current_name: Option<String> = None;
    let mut current_path: Option<String> = None;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_example
                && current_name.as_deref() == Some(example_name)
                && let Some(path) = current_path.take()
            {
                return Ok(app_dir.join(path));
            }
            in_example = trimmed == "[[example]]";
            if in_example {
                current_name = None;
                current_path = None;
            }
            continue;
        }
        if !in_example {
            continue;
        }
        if trimmed.starts_with("name") {
            if let Some((_, value)) = trimmed.split_once('=') {
                current_name = Some(value.trim().trim_matches('"').to_string());
            }
        } else if trimmed.starts_with("path")
            && let Some((_, value)) = trimmed.split_once('=')
        {
            current_path = Some(value.trim().trim_matches('"').to_string());
        }
    }

    if in_example
        && current_name.as_deref() == Some(example_name)
        && let Some(path) = current_path
    {
        return Ok(app_dir.join(path));
    }

    Err(format!(
        "missing path for example {example_name} in {}",
        manifest_path.display()
    ))
}
