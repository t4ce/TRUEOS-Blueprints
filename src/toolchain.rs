use std::path::PathBuf;
use std::process::Command;

/// Dated toolchain proven by the TRUEOS `std` build-source adaptations.
///
/// Keep this synchronized with the workspace `rust-toolchain.toml`. A dated
/// channel is intentional: the builder edits rust-src using revision-specific
/// source markers and must never follow the floating `nightly` alias.
pub(crate) const RUST_TOOLCHAIN: &str = "nightly-2026-07-10";
pub(crate) const RUSTC_COMMIT: &str = "af3d95584dbddcae597890340995509a7fb47a50";

pub(crate) fn cargo_command() -> Command {
    rustup_command("cargo")
}

pub(crate) fn rustc_command() -> Command {
    rustup_command("rustc")
}

fn rustup_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.arg(format!("+{RUST_TOOLCHAIN}"));
    command
}

pub(crate) fn verify_rustc_identity() -> Result<(), String> {
    let output = rustc_command().arg("-vV").output().map_err(|err| {
        format!(
            "failed to start rustc for pinned toolchain {RUST_TOOLCHAIN}: {err}; \
                 install it with `rustup toolchain install {RUST_TOOLCHAIN} \
                 --profile minimal --component rust-src`"
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "rustc for pinned toolchain {RUST_TOOLCHAIN} failed with status {}",
            output.status
        ));
    }

    let version = String::from_utf8(output.stdout)
        .map_err(|err| format!("pinned rustc version output was not UTF-8: {err}"))?;
    verify_rustc_version_text(&version)
}

pub(crate) fn rust_sysroot() -> Result<PathBuf, String> {
    let output = rustc_command()
        .arg("--print")
        .arg("sysroot")
        .output()
        .map_err(|err| format!("rustc +{RUST_TOOLCHAIN} --print sysroot failed to start: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc +{RUST_TOOLCHAIN} --print sysroot failed with status {}",
            output.status
        ));
    }
    let sysroot = String::from_utf8(output.stdout)
        .map_err(|err| format!("rustc sysroot output was not UTF-8: {err}"))?;
    Ok(PathBuf::from(sysroot.trim()))
}

fn verify_rustc_version_text(version: &str) -> Result<(), String> {
    let actual_commit = version
        .lines()
        .find_map(|line| line.strip_prefix("commit-hash: "))
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
        .ok_or_else(|| {
            format!(
                "rustc +{RUST_TOOLCHAIN} -vV did not report a commit hash; \
                 refusing to modify rust-src"
            )
        })?;
    if actual_commit != RUSTC_COMMIT {
        return Err(format!(
            "TRUEOS std port expects rustc commit {RUSTC_COMMIT} \
             ({RUST_TOOLCHAIN}), but rustup resolved {actual_commit}; \
             refusing to modify rust-src"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_pinned_rustc_commit() {
        let version = format!(
            "rustc 1.99.0-nightly\ncommit-hash: {RUSTC_COMMIT}\nhost: x86_64-unknown-linux-gnu\n"
        );
        assert!(verify_rustc_version_text(&version).is_ok());
    }

    #[test]
    fn rejects_a_different_nightly_commit() {
        let err = verify_rustc_version_text(
            "rustc 1.99.0-nightly\ncommit-hash: da86f4d0726be475afbbffe40cb2f65741c51ad3\n",
        )
        .unwrap_err();
        assert!(err.contains("refusing to modify rust-src"));
        assert!(err.contains(RUSTC_COMMIT));
    }

    #[test]
    fn workspace_toolchain_manifest_matches_the_builder_pin() {
        let manifest = include_str!("../rust-toolchain.toml");
        assert!(manifest.contains(&format!("channel = \"{RUST_TOOLCHAIN}\"")));
    }
}
