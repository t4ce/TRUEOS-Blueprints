//! Pinned OpenSSL crypto with native app-owned OS primitives.
use std::path::{Path, PathBuf};
const SOURCE: &str = "300.6.1+3.6.3";
const SYS: &str = "0.9.117";

pub(crate) fn patches(
    app: &Path,
    work: &Path,
    selected: &[crate::CratePatch],
) -> Result<Vec<crate::CratePatch>, String> {
    let packages = crate::lock_packages(&crate::app_lockfile(app))?;
    let mut patches = Vec::new();
    for (name, version) in [("openssl-src", SOURCE), ("openssl-sys", SYS)] {
        let found: Vec<_> = packages
            .iter()
            .filter(|p| {
                p.name == name
                    && p.source
                        .as_deref()
                        .is_some_and(|s| s.starts_with("registry+"))
            })
            .collect();
        if found.is_empty() {
            continue;
        }
        if found.len() != 1 || found[0].version != version {
            return Err(format!("native OpenSSL requires original {name}-{version}"));
        }
        // openssl-src's existing source-only overlay is deliberately extended
        // here. An application-supplied OpenSSL/sys source remains its owner.
        if name == "openssl-sys" && selected.iter().any(|p| p.name == name) {
            continue;
        }
        let source = crate::find_cargo_registry_crate(&format!("{name}-{version}"))
            .ok_or_else(|| format!("missing original native OpenSSL {name}-{version}"))?;
        let path = if name == "openssl-src" {
            stage_source(work, &source)?
        } else {
            stage_sys(work, &source)?
        };
        patches.push(crate::CratePatch::new(name, path));
    }
    Ok(patches)
}
fn replace(text: &mut String, before: &str, after: &str) -> Result<(), String> {
    if text.matches(before).count() != 1 {
        return Err(format!("pinned native OpenSSL boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
fn edit(
    root: &Path,
    path: &str,
    change: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let path = root.join(path);
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    change(&mut text)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}
pub(crate) fn stage_source(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let staged = crate::openssl_overlay::stage(work, source)?;
    edit(&staged, "src/lib.rs", |text| {
        replace(
            text,
            "        if target == \"x86_64-unknown-trueos\" {",
            "        if target == \"x86_64-unknown-trueos\" {\n            configure.arg(format!(\"-include{}\", inner_dir.join(\"include/internal/trueos_native.h\").display()));",
        )
    })?;
    edit(&staged, "src/lib.rs", |text| {
        replace(
            text,
            "\"--with-rand-seed=rdcpu\"",
            "\"--with-rand-seed=getrandom\"",
        )
    })?;
    edit(
        &staged,
        "openssl/providers/implementations/rands/seeding/rand_unix.c",
        |text| {
            replace(
                text,
                "static ssize_t syscall_random(void *buf, size_t buflen)\n{",
                "static ssize_t syscall_random(void *buf, size_t buflen)\n{\n#if defined(OPENSSL_SYS_TRUEOS)\n    return ossl_trueos_getentropy(buf, buflen) == 0 ? (ssize_t)buflen : -1;\n#else",
            )?;
            replace(
                text,
                "}\n#endif /* defined(OPENSSL_RAND_SEED_GETRANDOM) */",
                "#endif\n}\n#endif /* defined(OPENSSL_RAND_SEED_GETRANDOM) */",
            )?;
            replace(
                text,
                "#if defined(__linux) && defined(DEVRANDOM_WAIT) \\\n    && defined(OPENSSL_RAND_SEED_GETRANDOM)",
                "#if !defined(OPENSSL_SYS_TRUEOS) && defined(__linux) && defined(DEVRANDOM_WAIT) \\\n    && defined(OPENSSL_RAND_SEED_GETRANDOM)",
            )
        },
    )?;
    edit(&staged, "openssl/crypto/threads_pthread.c", |text| {
        replace(
            text,
            "_Static_assert(sizeof(pthread_mutex_t) == 40, \"native mutex ABI must be 40 bytes\");",
            "_Static_assert(sizeof(pthread_mutex_t) == sizeof(void *), \"native mutex is an app-owned opaque handle\");",
        )
    })?;
    edit(&staged, "openssl/crypto/mem.c", |text| {
        replace(
            text,
            "#if defined(_BSD_SOURCE) || (defined(_POSIX_C_SOURCE) && _POSIX_C_SOURCE >= 200112L)",
            "#if !defined(OPENSSL_SYS_TRUEOS) && (defined(_BSD_SOURCE) || (defined(_POSIX_C_SOURCE) && _POSIX_C_SOURCE >= 200112L))",
        )
    })?;
    edit(&staged, "openssl/crypto/o_str.c", |text| {
        replace(
            text,
            "#if defined(_MSC_VER) && _MSC_VER >= 1400 && !defined(_WIN32_WCE)",
            "#if defined(OPENSSL_SYS_TRUEOS)\n    return !ossl_trueos_strerror_r(errnum, buf, buflen);\n#elif defined(_MSC_VER) && _MSC_VER >= 1400 && !defined(_WIN32_WCE)",
        )
    })?;
    edit(
        &staged,
        "openssl/providers/implementations/storemgmt/file_store.c.in",
        |text| {
            replace(
                text,
                "static void *file_open(void *provctx, const char *uri)\n{",
                "#if !defined(OPENSSL_SYS_TRUEOS)\nstatic void *file_open(void *provctx, const char *uri)\n{",
            )?;
            replace(
                text,
                "void *file_attach(void *provctx, OSSL_CORE_BIO *cin)",
                "#else\nstatic void *file_open(void *provctx, const char *uri)\n{\n    /* The native profile already has no-stdio/no-posix-io. Memory BIO\n     * decoding remains available through file_attach below. */\n    ERR_raise_data(ERR_LIB_PROV, ERR_R_UNSUPPORTED, \"Native file URI loading is unavailable: %s\", uri);\n    return NULL;\n}\n#endif\n\nvoid *file_attach(void *provctx, OSSL_CORE_BIO *cin)",
            )
        },
    )?;
    std::fs::write(
        staged.join("openssl/include/internal/trueos_native.h"),
        include_str!("native_openssl_boundary.h"),
    )
    .map_err(|e| e.to_string())?;
    Ok(staged)
}
pub(crate) fn stage_sys(work: &Path, source: &Path) -> Result<PathBuf, String> {
    if crate::package_name(&source.join("Cargo.toml"))? != "openssl-sys"
        || crate::package_version(&source.join("Cargo.toml"))?.as_deref() != Some(SYS)
    {
        return Err(format!(
            "native OpenSSL requires original openssl-sys-{SYS}"
        ));
    }
    let staged = work
        .join("source-overlay-crates")
        .join(format!("openssl-sys-{SYS}-trueos-native"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    edit(&staged, "Cargo.toml", |text| {
        replace(text, "[features]", "[features]\ntrueos-native-testing = []")?;
        text.push_str(&format!("\n[target.'cfg(target_os = \"trueos\")'.dependencies.trueos]\npath = {:?}\ndefault-features = false\nfeatures = [\"native-c-alloc\"]\n",PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("api")));
        Ok(())
    })?;
    edit(&staged, "src/lib.rs", |text| {
        text.push_str(
            r#"
#[cfg(any(target_os = "trueos", feature = "trueos-native-testing"))]
mod native_openssl_boundary;
#[cfg(target_os = "trueos")]
use trueos::native_c_alloc as _;
#[cfg(all(not(target_os = "trueos"), feature = "trueos-native-testing"))]
extern crate alloc;
#[cfg(all(not(target_os = "trueos"), feature = "trueos-native-testing"))]
#[path = "native_openssl_allocator.rs"]
mod native_openssl_allocator;
"#,
        );
        Ok(())
    })?;
    std::fs::write(
        staged.join("src/native_openssl_boundary.rs"),
        include_str!("native_openssl_boundary.rs"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/native_openssl_allocator.rs"),
        include_str!("../crates/trueos-v/src/native_c_alloc.rs"),
    )
    .map_err(|e| e.to_string())?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stages_exact_original_openssl_sources_with_native_boundary() {
        let work =
            std::env::temp_dir().join(format!("native-openssl-staging-{}", std::process::id()));
        let source = crate::find_cargo_registry_crate(&format!("openssl-src-{SOURCE}")).unwrap();
        let sys = crate::find_cargo_registry_crate(&format!("openssl-sys-{SYS}")).unwrap();
        stage_source(&work, &source).unwrap();
        stage_sys(&work, &sys).unwrap();
        println!("native OpenSSL proof stage: {}", work.display());
    }
}
