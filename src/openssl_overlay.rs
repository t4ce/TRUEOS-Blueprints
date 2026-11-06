//! Native platform configuration for the vendored OpenSSL used by WebAuthn.
//!
//! Keep upstream crypto algorithms and x86 CPUID/RDSEED/RDRAND assembly. The
//! native entropy contract uses OpenSSL's built-in `rdcpu` seed source only:
//! CPUs without those instructions, or failed instruction retries, fail closed.
//! It never uses Linux syscalls, random-device files or the void sys_rand ABI.
//! Native synchronization uses the existing mutex backend plus acquire/release
//! once publication; it does not pretend POSIX thread lifecycle is available.

use std::path::{Path, PathBuf};

const RELEASE: &str = "300.6.1+3.6.3";
const BUILDER: &str = "src/lib.rs";
const THREADS: &str = "openssl/crypto/threads_pthread.c";
const UID: &str = "openssl/crypto/uid.c";
const DIRECTORY: &str = "openssl/crypto/o_dir.c";
const GETENV: &str = "openssl/crypto/getenv.c";
const RANDFILE: &str = "openssl/crypto/rand/randfile.c";
const COMMON_HEADER: &str = "openssl/include/internal/e_os.h";
const BUILD_INFO: &str = "openssl/crypto/build.info";
const COMPAT_SOURCE: &str = include_str!("openssl_trueos_compat.c");
const COMPAT_HEADER: &str = r#"/* Native utility calls use C-locale semantics, never libc Joker fallbacks. */
#if defined(OPENSSL_SYS_TRUEOS)
#ifndef OSSL_TRUEOS_COMPAT_H
#define OSSL_TRUEOS_COMPAT_H
/* Load the host declarations before remapping names; this prevents glibc's
 * C23 assembler aliases or fortified inline declarations renaming our calls. */
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
char *ossl_trueos_strcpy(char *, const char *);
char *ossl_trueos_strdup(const char *);
char *ossl_trueos_strncpy(char *, const char *, size_t);
char *ossl_trueos_strpbrk(const char *, const char *);
char *ossl_trueos_strstr(const char *, const char *);
long ossl_trueos_strtol(const char *, char **, int);
unsigned long ossl_trueos_strtoul(const char *, char **, int);
struct tm *ossl_trueos_gmtime_r(const time_t *, struct tm *);
int ossl_trueos_tolower(int);
/* Only the exact %d/%u HTTP-port formats used by this pinned release. */
int ossl_trueos_sscanf(const char *, const char *, ...);
#undef strcpy
#undef strdup
#undef strncpy
#undef strpbrk
#undef strstr
#undef strtol
#undef strtoul
#undef gmtime_r
#undef tolower
#undef sscanf
#define strcpy ossl_trueos_strcpy
#define strdup ossl_trueos_strdup
#define strncpy ossl_trueos_strncpy
#define strpbrk ossl_trueos_strpbrk
#define strstr ossl_trueos_strstr
#define strtol ossl_trueos_strtol
#define strtoul ossl_trueos_strtoul
#define gmtime_r ossl_trueos_gmtime_r
#define tolower ossl_trueos_tolower
#define sscanf ossl_trueos_sscanf
#endif
#endif
"#;
const TARGET_MATCH: &str = "        let os = match target {";
const NATIVE_TARGET: &str = r#"        if target == "x86_64-unknown-trueos" {
            // Native Hulls have no FILE*, DSO, Unix sockets, fork or mlock.
            // Keep thread safety; only the optional internal worker pool is
            // unavailable. CPUID and RDSEED/RDRAND assembly MUST stay enabled.
            configure.args([
                "no-stdio", "no-posix-io", "no-ui-console", "no-dso",
                "no-engine", "no-async", "no-sock", "no-thread-pool",
                "no-atexit", "no-autoload-config", "no-afalgeng",
                "-DOPENSSL_NO_SECURE_MEMORY", "-DOPENSSL_SYS_TRUEOS",
                "-DNO_SYSLOG",
                "--with-rand-seed=rdcpu",
            ]);
        }

        let os = match target {
            "x86_64-unknown-trueos" => "linux-x86_64","#;
const RWLOCK: &str = "    && !defined(__riscv)\n#define USE_RWLOCK";
const NATIVE_MUTEX: &str =
    "    && !defined(__riscv) && !defined(OPENSSL_SYS_TRUEOS)\n#define USE_RWLOCK";
const ONCE: &str = r#"int CRYPTO_THREAD_run_once(CRYPTO_ONCE *once, void (*init)(void))
{
    if (ossl_unlikely(pthread_once(once, init) != 0))
        return 0;

    return 1;
}"#;
const NATIVE_ONCE: &str = r#"int CRYPTO_THREAD_run_once(CRYPTO_ONCE *once, void (*init)(void))
{
#if defined(OPENSSL_SYS_TRUEOS)
    /* Native threads are not POSIX threads. Publish initialization through
     * real lock-free atomics; yield while its owner runs on another carrier.
     * Native threads do not have POSIX asynchronous cancellation. */
    int expected = 0;
    _Static_assert(sizeof(*once) == sizeof(int), "native once ABI must be an int");
    _Static_assert(PTHREAD_ONCE_INIT == 0, "native once must start uninitialized");
    if (__atomic_load_n(once, __ATOMIC_ACQUIRE) == 2)
        return 1;
    if (__atomic_compare_exchange_n(once, &expected, 1, 0,
            __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        init();
        __atomic_store_n(once, 2, __ATOMIC_RELEASE);
    } else {
        while (__atomic_load_n(once, __ATOMIC_ACQUIRE) != 2)
            sched_yield();
    }
    return 1;
#else
    if (ossl_unlikely(pthread_once(once, init) != 0))
        return 0;

    return 1;
#endif
}"#;
const COMPARE_ID: &str = "    return pthread_equal(a, b);";
const NATIVE_COMPARE_ID: &str = "#if defined(OPENSSL_SYS_TRUEOS)\n    return a == b; /* Native pthread_self supplies an opaque integral ID. */\n#else\n    return pthread_equal(a, b);\n#endif";
const UID_PLATFORMS: &str = "#if defined(OPENSSL_SYS_WIN32) || defined(OPENSSL_SYS_VXWORKS) || defined(OPENSSL_SYS_UEFI) || defined(__wasi__)";
const NATIVE_UID: &str = "/* Native Hull launches have no Unix setuid/setgid exec transition. */\n#if defined(OPENSSL_SYS_TRUEOS) || defined(OPENSSL_SYS_WIN32) || defined(OPENSSL_SYS_VXWORKS) || defined(OPENSSL_SYS_UEFI) || defined(__wasi__)";

fn replace_once(text: &str, needle: &str, replacement: &str, file: &str) -> Result<String, String> {
    let count = text.matches(needle).count();
    if count != 1 {
        return Err(format!(
            "OpenSSL native patch expected one original template in {file}; found {count}"
        ));
    }
    Ok(text.replacen(needle, replacement, 1))
}

pub(crate) fn stage(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "openssl-src"
        || crate::package_version(&manifest)?.as_deref() != Some(RELEASE)
    {
        return Err(format!(
            "OpenSSL native staging requires original openssl-src {RELEASE}: {}",
            source.display()
        ));
    }
    // Validate every original source before replacing a previous good stage.
    let read = |file: &str| {
        std::fs::read_to_string(source.join(file))
            .map_err(|error| format!("read {}: {error}", source.join(file).display()))
    };
    let builder = replace_once(&read(BUILDER)?, TARGET_MATCH, NATIVE_TARGET, BUILDER)?;
    let threads = replace_once(&read(THREADS)?, RWLOCK, NATIVE_MUTEX, THREADS)?;
    let threads = replace_once(&threads, ONCE, NATIVE_ONCE, THREADS)?;
    let threads = replace_once(&threads, COMPARE_ID, NATIVE_COMPARE_ID, THREADS)?;
    let threads = replace_once(
        &threads,
        "#include <assert.h>",
        "#include <assert.h>\n#if defined(OPENSSL_SYS_TRUEOS)\n#include <sched.h>\n_Static_assert(sizeof(pthread_mutex_t) == 40, \"native mutex ABI must be 40 bytes\");\n_Static_assert(sizeof(pthread_key_t) == 4, \"native TLS key ABI must be 32 bits\");\n_Static_assert(sizeof(pthread_t) == 8, \"native thread identity ABI must be 64 bits\");\n#endif",
        THREADS,
    )?;
    let uid = replace_once(&read(UID)?, UID_PLATFORMS, NATIVE_UID, UID)?;
    let directory = replace_once(
        &read(DIRECTORY)?,
        "#if defined OPENSSL_SYS_UNIX || defined DJGPP",
        "#if defined OPENSSL_SYS_TRUEOS\n# include \"LPdir_nyi.c\"\n#elif defined OPENSSL_SYS_UNIX || defined DJGPP",
        DIRECTORY,
    )?;
    let getenv = replace_once(
        &read(GETENV)?,
        "#if defined(__GLIBC__) && defined(__GLIBC_PREREQ)",
        "#if !defined(OPENSSL_SYS_TRUEOS) && defined(__GLIBC__) && defined(__GLIBC_PREREQ)",
        GETENV,
    )?;
    let randfile = native_seed_file_errors(&read(RANDFILE)?)?;
    let header = read(COMMON_HEADER)?;
    let Some(header) = header.strip_suffix("\n#endif\n") else {
        return Err(format!("missing original final include guard in {COMMON_HEADER}"));
    };
    let header = format!("{header}\n#include \"internal/trueos_compat.h\"\n\n#endif\n");
    let build_info = replace_once(
        &read(BUILD_INFO)?,
        "SOURCE[../libcrypto]=$CPUID_COMMON\n",
        "SOURCE[../libcrypto]=$CPUID_COMMON trueos_compat.c\n",
        BUILD_INFO,
    )?;
    let staged = work_dir
        .join("source-overlay-crates")
        .join(format!("openssl-src-{RELEASE}-trueos"));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    for (file, contents) in [
        (BUILDER, builder),
        (THREADS, threads),
        (UID, uid),
        (DIRECTORY, directory),
        (GETENV, getenv),
        (RANDFILE, randfile),
        (COMMON_HEADER, header),
        (BUILD_INFO, build_info),
        ("openssl/include/internal/trueos_compat.h", COMPAT_HEADER.to_owned()),
        ("openssl/crypto/trueos_compat.c", COMPAT_SOURCE.to_owned()),
    ] {
        std::fs::write(staged.join(file), contents)
            .map_err(|error| format!("write {}: {error}", staged.join(file).display()))?;
    }
    Ok(staged)
}

fn native_seed_file_errors(text: &str) -> Result<String, String> {
    // With no-stdio, upstream openssl_fopen always returns NULL. Make those
    // existing error paths explicit so unreachable FILE* calls do not remain
    // unresolved imports in a native static archive. This is not an entropy
    // fallback: the sole configured source remains hardware RDCPU.
    let load = text.find("int RAND_load_file(const char *file, long bytes)");
    let write = text.find("int RAND_write_file(const char *file)");
    let name = text.find("const char *RAND_file_name(char *buf, size_t size)");
    let (Some(load), Some(write), Some(name)) = (load, write, name) else {
        return Err(format!(
            "missing original seed-file functions in {RANDFILE}"
        ));
    };
    if !(load < write && write < name) {
        return Err(format!("unexpected seed-file function order in {RANDFILE}"));
    }
    Ok(format!(
        "{}#if !defined(OPENSSL_SYS_TRUEOS)\n{}#else\nint RAND_load_file(const char *file, long bytes)\n{{\n    if (bytes == 0) return 0;\n    ERR_raise_data(ERR_LIB_RAND, RAND_R_CANNOT_OPEN_FILE, \"Filename=%s\", file);\n    return -1;\n}}\nint RAND_write_file(const char *file)\n{{\n    ERR_raise_data(ERR_LIB_RAND, RAND_R_CANNOT_OPEN_FILE, \"Filename=%s\", file);\n    return -1;\n}}\n#endif\n{}",
        &text[..load],
        &text[load..name],
        &text[name..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> (PathBuf, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "trueos-openssl-stage-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        for file in [BUILDER, THREADS, UID, DIRECTORY, GETENV, RANDFILE, COMMON_HEADER, BUILD_INFO] {
            fs::create_dir_all(source.join(file).parent().unwrap()).unwrap();
        }
        fs::write(source.join("Cargo.toml"), format!("[package]\nname=\"openssl-src\"\nversion=\"{RELEASE}\"\n\n[features]\ndefault=[\"legacy\"]\nlegacy=[]\n")).unwrap();
        fs::write(
            source.join(BUILDER),
            format!("// before\n{TARGET_MATCH}\n// after\n"),
        )
        .unwrap();
        fs::write(
            source.join(DIRECTORY),
            "#if defined OPENSSL_SYS_UNIX || defined DJGPP\n",
        )
        .unwrap();
        fs::write(
            source.join(GETENV),
            "#if defined(__GLIBC__) && defined(__GLIBC_PREREQ)\n",
        )
        .unwrap();
        fs::write(source.join(RANDFILE), "int RAND_load_file(const char *file, long bytes) { return 1; }\nint RAND_write_file(const char *file) { return 1; }\nconst char *RAND_file_name(char *buf, size_t size) { return buf; }\n").unwrap();
        fs::write(source.join(COMMON_HEADER), "#ifndef OSSL_E_OS_H\n#define OSSL_E_OS_H\n// original header\n#endif\n").unwrap();
        fs::write(source.join(BUILD_INFO), "SOURCE[../libcrypto]=$CPUID_COMMON\n").unwrap();
        fs::write(
            source.join(THREADS),
            format!("#include <assert.h>\n{RWLOCK}\n{ONCE}\n{COMPARE_ID}\n// retained\n"),
        )
        .unwrap();
        fs::write(source.join(UID), format!("{UID_PLATFORMS}\n// retained\n")).unwrap();
        fs::write(source.join("openssl/unchanged"), b"unmodified algorithms").unwrap();
        (root, source)
    }

    #[test]
    fn preserves_release_features_and_algorithms_while_selecting_native_platform() {
        let (root, source) = fixture();
        let staged = stage(&root.join("work"), &source).unwrap();
        for file in ["Cargo.toml", "openssl/unchanged"] {
            assert_eq!(
                fs::read(staged.join(file)).unwrap(),
                fs::read(source.join(file)).unwrap()
            );
        }
        let builder = fs::read_to_string(staged.join(BUILDER)).unwrap();
        assert_eq!(builder, format!("// before\n{NATIVE_TARGET}\n// after\n"));
        assert!(!builder.contains("no-threads\""));
        assert!(!builder.contains("no-asm\""));
        assert_eq!(builder.matches("--with-rand-seed=rdcpu").count(), 1);
        let threads = fs::read_to_string(staged.join(THREADS)).unwrap();
        assert!(threads.contains(NATIVE_ONCE));
        assert!(threads.contains(NATIVE_COMPARE_ID));
        assert!(threads.contains(NATIVE_MUTEX));
        assert_eq!(fs::read_to_string(staged.join("openssl/crypto/trueos_compat.c")).unwrap(), COMPAT_SOURCE);
        assert_eq!(fs::read_to_string(staged.join("openssl/include/internal/trueos_compat.h")).unwrap(), COMPAT_HEADER);
        assert!(
            fs::read_to_string(source.join(THREADS))
                .unwrap()
                .contains(ONCE)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_original_is_refused_before_resetting_existing_stage() {
        let (root, source) = fixture();
        let work = root.join("work");
        let staged = work
            .join("source-overlay-crates")
            .join(format!("openssl-src-{RELEASE}-trueos"));
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("retained"), "previous good stage").unwrap();
        fs::write(source.join(THREADS), "unexpected upstream source").unwrap();
        assert!(stage(&work, &source).unwrap_err().contains(THREADS));
        assert_eq!(
            fs::read_to_string(staged.join("retained")).unwrap(),
            "previous good stage"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_once_initializes_once_and_publishes_to_concurrent_callers() {
        let (root, _) = fixture();
        let test = root.join("once.c");
        fs::write(
            &test,
            format!(
                r#"
#include <pthread.h>
#include <sched.h>
#include <assert.h>
typedef pthread_once_t CRYPTO_ONCE;
{NATIVE_ONCE}
static CRYPTO_ONCE once = PTHREAD_ONCE_INIT;
static int calls, published;
static void initialize(void) {{ __atomic_add_fetch(&calls, 1, __ATOMIC_RELAXED); published = 42; }}
static void *worker(void *arg) {{
    (void)arg;
    for (int i = 0; i < 1000; ++i) {{
        assert(CRYPTO_THREAD_run_once(&once, initialize) == 1);
        assert(published == 42);
    }}
    return 0;
}}
int main(void) {{
    pthread_t threads[16];
    for (int i = 0; i < 16; ++i) assert(pthread_create(&threads[i], 0, worker, 0) == 0);
    for (int i = 0; i < 16; ++i) assert(pthread_join(threads[i], 0) == 0);
    assert(calls == 1);
    return 0;
}}
"#
            ),
        )
        .unwrap();
        let executable = root.join("once");
        let compile = std::process::Command::new("cc")
            .args(["-std=c11", "-DOPENSSL_SYS_TRUEOS", "-pthread"])
            .arg(&test)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(
            std::process::Command::new(executable)
                .status()
                .unwrap()
                .success()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
