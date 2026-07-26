use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Dated toolchain proven by the TRUEOS `std` and compiler-source adaptations.
///
/// Keep this synchronized with the workspace `rust-toolchain.toml`. A dated
/// channel is intentional: the builder edits rust-src and compiles rustc-dev
/// sources using revision-specific source markers, so it must never follow the
/// floating `nightly` alias.
pub(crate) const RUST_TOOLCHAIN: &str = "nightly-2026-07-10";
pub(crate) const RUSTC_COMMIT: &str = "af3d95584dbddcae597890340995509a7fb47a50";
pub(crate) const RUST_TOOLCHAIN_ROOT_ENV: &str = "TRUEOS_RUST_TOOLCHAIN_ROOT";

const ARCHIVED_TOOLCHAIN_DIR: &str = "TRUEOS-Rust-Toolchain-nightly-2026-07-10";
const RUSTC_SOURCE_RELATIVE: &str = "lib/rustlib/rustc-src/rust";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustcIdentity {
    /// The text printed after `rustc` on the first line of `rustc -vV`.
    pub(crate) version: String,
    pub(crate) commit_hash: String,
    pub(crate) commit_date: String,
    pub(crate) host: String,
    pub(crate) release: String,
    pub(crate) release_channel: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustcSourceLayout {
    /// Root of the matching rustc-dev source extraction.
    pub(crate) root: PathBuf,
    /// The `compiler` directory containing the rustc-private crates.
    pub(crate) compiler: PathBuf,
    pub(crate) cargo_lock: PathBuf,
    pub(crate) driver_impl_manifest: PathBuf,
    pub(crate) cranelift_manifest: PathBuf,
    pub(crate) identity: RustcIdentity,
}

pub(crate) fn cargo_command() -> Command {
    let root = configured_toolchain_root();
    cargo_command_for(root.as_deref())
}

fn cargo_command_for(root: Option<&Path>) -> Command {
    if let Some(root) = root {
        let mut command = direct_command(root, "cargo");
        // Cargo otherwise finds rustc through PATH, which can silently mix the
        // archived Cargo with a different rustup toolchain.
        command.env("RUSTC", tool_path(root, "rustc"));
        command
    } else {
        rustup_command("cargo")
    }
}

pub(crate) fn rustc_command() -> Command {
    let root = configured_toolchain_root();
    rustc_command_for(root.as_deref())
}

fn rustc_command_for(root: Option<&Path>) -> Command {
    root.map_or_else(
        || rustup_command("rustc"),
        |root| direct_command(root, "rustc"),
    )
}

/// Returns the explicitly configured archive, or the adjacent archive when the
/// Blueprint checkout and toolchain checkout share a repository parent.
///
/// An explicit path always wins, including when it is invalid. That makes a
/// misspelled `TRUEOS_RUST_TOOLCHAIN_ROOT` fail closed instead of falling back
/// to a coincidentally installed rustup toolchain.
pub(crate) fn configured_toolchain_root() -> Option<PathBuf> {
    let explicit = env::var_os(RUST_TOOLCHAIN_ROOT_ENV);
    let sibling = sibling_toolchain_root(Path::new(env!("CARGO_MANIFEST_DIR")));
    choose_toolchain_root(explicit, sibling).map(absolutize)
}

/// Resolves the selected compiler's canonical sysroot.
///
/// For a direct archive selection, rustc must report the selected archive as
/// its sysroot. This prevents a wrapper or misplaced binary from pairing a
/// compiler with unrelated rustc-dev sources.
pub(crate) fn toolchain_root() -> Result<PathBuf, String> {
    rust_sysroot()
}

pub(crate) fn verify_rustc_identity() -> Result<(), String> {
    rustc_identity().map(|_| ())
}

pub(crate) fn rustc_identity() -> Result<RustcIdentity, String> {
    validate_configured_toolchain_root()?;

    let output = rustc_command().arg("-vV").output().map_err(|err| {
        if let Some(root) = configured_toolchain_root() {
            format!(
                "failed to start rustc from {} selected by {RUST_TOOLCHAIN_ROOT_ENV} \
                 or the adjacent toolchain archive: {err}",
                root.display()
            )
        } else {
            format!(
                "failed to start rustc for pinned toolchain {RUST_TOOLCHAIN}: {err}; \
                 install it with `rustup toolchain install {RUST_TOOLCHAIN} \
                 --profile minimal --component rust-src` or set \
                 {RUST_TOOLCHAIN_ROOT_ENV}"
            )
        }
    })?;
    if !output.status.success() {
        return Err(format!(
            "{} -vV failed with status {}",
            rustc_description(),
            output.status
        ));
    }

    let version = String::from_utf8(output.stdout)
        .map_err(|err| format!("selected rustc version output was not UTF-8: {err}"))?;
    parse_rustc_identity(&version)
}

pub(crate) fn rust_sysroot() -> Result<PathBuf, String> {
    validate_configured_toolchain_root()?;

    let output = rustc_command()
        .arg("--print")
        .arg("sysroot")
        .output()
        .map_err(|err| {
            format!(
                "{} --print sysroot failed to start: {err}",
                rustc_description()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "{} --print sysroot failed with status {}",
            rustc_description(),
            output.status
        ));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|err| format!("rustc sysroot output was not UTF-8: {err}"))?;
    let reported = output.trim();
    if reported.is_empty() {
        return Err(format!(
            "{} --print sysroot returned an empty path",
            rustc_description()
        ));
    }
    let sysroot = canonicalize_existing(Path::new(reported), "rustc sysroot")?;

    if let Some(configured) = configured_toolchain_root() {
        let configured = canonicalize_existing(&configured, "configured Rust toolchain root")?;
        if sysroot != configured {
            return Err(format!(
                "rustc at {} reports sysroot {}, not its selected toolchain root {}; \
                 refusing to mix compiler binaries and sources",
                tool_path(&configured, "rustc").display(),
                sysroot.display(),
                configured.display()
            ));
        }
    }

    Ok(sysroot)
}

/// Locates the compiler source shipped with the verified rustc archive.
pub(crate) fn rustc_source_layout() -> Result<RustcSourceLayout, String> {
    let identity = rustc_identity()?;
    let sysroot = rust_sysroot()?;
    let mut layout = rustc_source_layout_at(&sysroot)?;
    layout.identity = identity;
    Ok(layout)
}

pub(crate) fn rustc_source_root() -> Result<PathBuf, String> {
    rustc_source_layout().map(|layout| layout.root)
}

pub(crate) fn rustc_compiler_source() -> Result<PathBuf, String> {
    rustc_source_layout().map(|layout| layout.compiler)
}

/// Applies the rustc bootstrap identity expected by the compiler crates in the
/// matching rustc-dev source tree.
///
/// `compiler_host` is the triple on which the newly built compiler will run,
/// such as `x86_64-unknown-trueos`. It is intentionally distinct from the
/// verified stage-0 compiler's Linux build triple. This helper does not choose
/// a codegen backend or Cargo target; callers remain responsible for those
/// build-policy decisions.
pub(crate) fn configure_rustc_bootstrap_env(
    command: &mut Command,
    compiler_host: &str,
) -> Result<RustcIdentity, String> {
    if compiler_host.trim().is_empty() {
        return Err("native rustc compiler-host triple must not be empty".to_owned());
    }
    let identity = rustc_identity()?;
    apply_rustc_bootstrap_env(command, &identity, compiler_host);
    Ok(identity)
}

fn apply_rustc_bootstrap_env(command: &mut Command, identity: &RustcIdentity, compiler_host: &str) {
    command
        .env("RUSTC_BOOTSTRAP", "1")
        .env("CFG_RELEASE", &identity.release)
        .env("CFG_RELEASE_CHANNEL", &identity.release_channel)
        .env("CFG_VERSION", &identity.version)
        .env("CFG_VER_HASH", &identity.commit_hash)
        .env("CFG_VER_DATE", &identity.commit_date)
        .env("CFG_COMPILER_BUILD_TRIPLE", &identity.host)
        .env("CFG_COMPILER_HOST_TRIPLE", compiler_host)
        .env("RUSTC_INSTALL_BINDIR", "bin")
        .env("CFG_LIBDIR_RELATIVE", "lib");
}

fn rustup_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.arg(format!("+{RUST_TOOLCHAIN}"));
    command
}

fn direct_command(root: &Path, program: &str) -> Command {
    let mut command = Command::new(tool_path(root, program));

    // `cargo run` places its rustup sysroot first in LD_LIBRARY_PATH. Without
    // this override, the directly invoked archive rustc can load rustup's
    // librustc_driver and consequently report the rustup sysroot. Keep any
    // caller-provided paths, but make the selected archive authoritative.
    let archive_lib = root.join("lib");
    let mut library_paths = vec![archive_lib.clone()];
    if let Some(inherited) = env::var_os("LD_LIBRARY_PATH") {
        library_paths.extend(env::split_paths(&inherited).filter(|path| path != &archive_lib));
    }
    let library_path =
        env::join_paths(library_paths).unwrap_or_else(|_| archive_lib.into_os_string());
    command.env("LD_LIBRARY_PATH", library_path);
    command
}

fn tool_path(root: &Path, program: &str) -> PathBuf {
    root.join("bin").join(program)
}

fn validate_configured_toolchain_root() -> Result<(), String> {
    let Some(root) = configured_toolchain_root() else {
        return Ok(());
    };

    for program in ["cargo", "rustc"] {
        let path = tool_path(&root, program);
        if !path.is_file() {
            return Err(format!(
                "selected Rust toolchain root {} does not contain {}; \
                 fix {RUST_TOOLCHAIN_ROOT_ENV}",
                root.display(),
                path.display()
            ));
        }
    }
    Ok(())
}

fn choose_toolchain_root(explicit: Option<OsString>, sibling: Option<PathBuf>) -> Option<PathBuf> {
    explicit
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or(sibling)
}

fn sibling_toolchain_root(manifest_dir: &Path) -> Option<PathBuf> {
    let candidate = manifest_dir.parent()?.join(ARCHIVED_TOOLCHAIN_DIR);
    (tool_path(&candidate, "cargo").is_file() && tool_path(&candidate, "rustc").is_file())
        .then_some(candidate)
}

fn absolutize(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir().map_or(path.clone(), |current| current.join(path))
    }
}

fn canonicalize_existing(path: &Path, label: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|err| format!("failed to resolve {label} {}: {err}", path.display()))
}

fn rustc_description() -> String {
    configured_toolchain_root().map_or_else(
        || format!("rustc +{RUST_TOOLCHAIN}"),
        |root| tool_path(&root, "rustc").display().to_string(),
    )
}

fn parse_rustc_identity(version: &str) -> Result<RustcIdentity, String> {
    let version_text = version
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("rustc "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "{} -vV did not report its version on the first line",
                rustc_description()
            )
        })?;
    let commit_hash = version_field(version, "commit-hash: ")?;
    if commit_hash != RUSTC_COMMIT {
        return Err(format!(
            "TRUEOS compiler port expects rustc commit {RUSTC_COMMIT} \
             ({RUST_TOOLCHAIN}), but the selected rustc reported {commit_hash}; \
             refusing to modify rust-src or compile rustc-dev sources"
        ));
    }

    let commit_date = version_field(version, "commit-date: ")?;
    let host = version_field(version, "host: ")?;
    let release = version_field(version, "release: ")?;
    let release_channel = release
        .rsplit_once('-')
        .map_or("stable", |(_, channel)| channel)
        .to_owned();

    Ok(RustcIdentity {
        version: version_text.to_owned(),
        commit_hash: commit_hash.to_owned(),
        commit_date: commit_date.to_owned(),
        host: host.to_owned(),
        release: release.to_owned(),
        release_channel,
    })
}

fn version_field<'a>(version: &'a str, prefix: &str) -> Result<&'a str, String> {
    version
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "{} -vV did not report {}",
                rustc_description(),
                prefix.trim_end()
            )
        })
}

fn verify_rustc_version_text(version: &str) -> Result<(), String> {
    parse_rustc_identity(version).map(|_| ())
}

fn rustc_source_layout_at(sysroot: &Path) -> Result<RustcSourceLayout, String> {
    let root = sysroot.join(RUSTC_SOURCE_RELATIVE);
    let compiler = root.join("compiler");
    let cargo_lock = root.join("Cargo.lock");
    let driver_impl_manifest = compiler.join("rustc_driver_impl/Cargo.toml");
    let cranelift_manifest = compiler.join("rustc_codegen_cranelift/Cargo.toml");
    let required = [
        &cargo_lock,
        &driver_impl_manifest,
        &cranelift_manifest,
        &compiler.join("rustc_interface/Cargo.toml"),
        &compiler.join("rustc_session/Cargo.toml"),
    ];

    let missing = required
        .iter()
        .filter(|path| !path.is_file())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "rustc {} is the expected commit, but its matching rustc-dev source \
             tree is incomplete under {}; missing: {}",
            RUSTC_COMMIT,
            root.display(),
            missing.join(", ")
        ));
    }

    Ok(RustcSourceLayout {
        root,
        compiler,
        cargo_lock,
        driver_impl_manifest,
        cranelift_manifest,
        // The public entry point replaces this after verifying the selected
        // compiler. Keeping the filesystem validator pure makes it testable.
        identity: RustcIdentity {
            version: String::new(),
            commit_hash: String::new(),
            commit_date: String::new(),
            host: String::new(),
            release: String::new(),
            release_channel: String::new(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const PINNED_VERSION: &str = "\
rustc 1.99.0-nightly (af3d95584 2026-07-09)
binary: rustc
commit-hash: af3d95584dbddcae597890340995509a7fb47a50
commit-date: 2026-07-09
host: x86_64-unknown-linux-gnu
release: 1.99.0-nightly
LLVM version: 22.1.8
";

    #[test]
    fn accepts_and_parses_the_pinned_rustc_commit() {
        let identity = parse_rustc_identity(PINNED_VERSION).unwrap();
        assert_eq!(
            identity,
            RustcIdentity {
                version: "1.99.0-nightly (af3d95584 2026-07-09)".to_owned(),
                commit_hash: RUSTC_COMMIT.to_owned(),
                commit_date: "2026-07-09".to_owned(),
                host: "x86_64-unknown-linux-gnu".to_owned(),
                release: "1.99.0-nightly".to_owned(),
                release_channel: "nightly".to_owned(),
            }
        );
        assert!(verify_rustc_version_text(PINNED_VERSION).is_ok());
    }

    #[test]
    fn rejects_a_different_nightly_commit() {
        let err = parse_rustc_identity(
            "\
rustc 1.99.0-nightly (da86f4d07 2026-07-08)
commit-hash: da86f4d0726be475afbbffe40cb2f65741c51ad3
commit-date: 2026-07-08
host: x86_64-unknown-linux-gnu
release: 1.99.0-nightly
",
        )
        .unwrap_err();
        assert!(err.contains("refusing to modify rust-src"));
        assert!(err.contains(RUSTC_COMMIT));
    }

    #[test]
    fn explicit_root_wins_and_empty_root_uses_the_sibling() {
        let sibling = PathBuf::from("/workspace/toolchain");
        assert_eq!(
            choose_toolchain_root(
                Some(OsString::from("/explicit/toolchain")),
                Some(sibling.clone())
            ),
            Some(PathBuf::from("/explicit/toolchain"))
        );
        assert_eq!(
            choose_toolchain_root(Some(OsString::new()), Some(sibling.clone())),
            Some(sibling)
        );
    }

    #[test]
    fn direct_commands_do_not_use_rustup_selectors() {
        let root = Path::new("/workspace/toolchain");
        let command = cargo_command_for(Some(root));
        assert_eq!(
            command.get_program(),
            OsStr::new("/workspace/toolchain/bin/cargo")
        );
        assert_eq!(command.get_args().count(), 0);
        assert_eq!(
            command
                .get_envs()
                .find_map(|(key, value)| (key == "RUSTC").then_some(value))
                .flatten(),
            Some(OsStr::new("/workspace/toolchain/bin/rustc"))
        );

        let fallback = cargo_command_for(None);
        assert_eq!(fallback.get_program(), OsStr::new("cargo"));
        assert_eq!(
            fallback.get_args().collect::<Vec<_>>(),
            [OsStr::new("+nightly-2026-07-10")]
        );
    }

    #[test]
    fn bootstrap_environment_carries_the_verified_identity() {
        let identity = parse_rustc_identity(PINNED_VERSION).unwrap();
        let mut command = Command::new("cargo");
        apply_rustc_bootstrap_env(&mut command, &identity, "x86_64-unknown-trueos");
        let environment = command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(environment[OsStr::new("RUSTC_BOOTSTRAP")], "1");
        assert_eq!(environment[OsStr::new("CFG_VER_HASH")], RUSTC_COMMIT);
        assert_eq!(
            environment[OsStr::new("CFG_VERSION")],
            "1.99.0-nightly (af3d95584 2026-07-09)"
        );
        assert_eq!(
            environment[OsStr::new("CFG_COMPILER_HOST_TRIPLE")],
            "x86_64-unknown-trueos"
        );
        assert_eq!(
            environment[OsStr::new("CFG_COMPILER_BUILD_TRIPLE")],
            "x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn locates_the_rustc_dev_compiler_tree() {
        let sysroot = test_directory("source-layout");
        let compiler = sysroot.join(RUSTC_SOURCE_RELATIVE).join("compiler");
        for relative in [
            "../Cargo.lock",
            "rustc_driver_impl/Cargo.toml",
            "rustc_codegen_cranelift/Cargo.toml",
            "rustc_interface/Cargo.toml",
            "rustc_session/Cargo.toml",
        ] {
            let path = compiler.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }

        let layout = rustc_source_layout_at(&sysroot).unwrap();
        assert_eq!(layout.compiler, compiler);
        assert_eq!(
            layout.driver_impl_manifest,
            compiler.join("rustc_driver_impl/Cargo.toml")
        );
        assert_eq!(
            layout.cranelift_manifest,
            compiler.join("rustc_codegen_cranelift/Cargo.toml")
        );

        fs::remove_dir_all(sysroot).unwrap();
    }

    #[test]
    fn workspace_toolchain_manifest_matches_the_builder_pin() {
        let manifest = include_str!("../rust-toolchain.toml");
        assert!(manifest.contains(&format!("channel = \"{RUST_TOOLCHAIN}\"")));
        assert!(ARCHIVED_TOOLCHAIN_DIR.ends_with(RUST_TOOLCHAIN));
    }

    #[test]
    fn adjacent_archive_is_coherent_when_present() {
        if env::var_os(RUST_TOOLCHAIN_ROOT_ENV).is_some() {
            // An explicit selection is user-controlled and is covered by the
            // command/path unit tests above.
            return;
        }
        let Some(adjacent) = sibling_toolchain_root(Path::new(env!("CARGO_MANIFEST_DIR"))) else {
            return;
        };

        let root = toolchain_root().unwrap();
        assert_eq!(root, adjacent.canonicalize().unwrap());
        assert_eq!(rustc_identity().unwrap().commit_hash, RUSTC_COMMIT);

        let layout = rustc_source_layout().unwrap();
        assert_eq!(layout.identity.commit_hash, RUSTC_COMMIT);
        assert_eq!(rustc_source_root().unwrap(), layout.root);
        assert_eq!(rustc_compiler_source().unwrap(), layout.compiler);

        let mut command = Command::new("cargo");
        let identity =
            configure_rustc_bootstrap_env(&mut command, "x86_64-unknown-trueos").unwrap();
        assert_eq!(identity.commit_hash, RUSTC_COMMIT);
    }

    fn test_directory(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "trueos-blueprint-toolchain-test-{}-{label}-{unique}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        path
    }
}
