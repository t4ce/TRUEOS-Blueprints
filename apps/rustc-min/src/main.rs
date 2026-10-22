#![feature(rustc_private)]
#![cfg_attr(not(test), no_main)]

mod archive_source;
mod blueprint;
mod blueprint_object;
mod git_source;
mod source_commands;
mod source_db;
mod source_loader;

use core::ffi::c_char;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

const ARGV0: &str = "rustc-min";
const MAX_COMPILER_ARGS: usize = 4096;
const MAX_COMPILER_ARG_BYTES: usize = 1024 * 1024;
const WORKERS: usize = 1;
const TIER: &str = "min";
const TARGET: &str = "rustc-sysroot/x86_64-unknown-trueos.json";
const TARGET_TRIPLE: &str = "x86_64-unknown-trueos";
const SYSROOT: &str = "rustc-sysroot";
const SYSROOT_MANIFEST: &str = "rustc-sysroot/TRUEOS_SYSROOT_MANIFEST";
const PAYLOAD_LIB: &str = "rustc-payload/lib";
const LOCAL_COMPILE_DIR: &str = "/common/localcompile";
const LOCAL_SOURCE: &str = "/common/localcompile/main.rs";
const LOCAL_OBJECT: &str = "/common/localcompile/local.o";
const MAX_LOCAL_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
static COMPILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const MANAGED_LOCAL_SOURCE_V1: &str = r#"#![no_std]
#![no_main]

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    let message: &[u8] = b"hello_world: rustc\n";
    unsafe {
        trueos_cabi_write(1, message.as_ptr(), message.len());
    }
}
"#;
const MANAGED_LOCAL_SOURCE_V2: &str = r#"#![no_std]
#![no_main]

use trueos::vsys;

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    vsys::log_info("hello_world: rustc trueos-api\n");
}
"#;
const MANAGED_LOCAL_SOURCE_V3: &str = r#"#![no_std]
#![no_main]

use trueos::logl::{self, level};

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    logl::log(level::INFO, "hello_world: rustc logl");
}
"#;
const MANAGED_LOCAL_SOURCE_V4: &str = r#"#![no_std]
#![no_main]

use trueos::vsys;

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    vsys::log_info("hello_world: rustc\n");
}
"#;
const GRIDPAPER_APP_SOURCE: &str =
    include_str!(concat!(
        env!("TRUEOS_BLUEPRINT_CATALOG_ROOT"),
        "/apps/gridpaper/src/rustc_oneshot.rs"
    ));

struct CompilerCallbacks {
    file_loader: Option<Box<dyn rustc_span::source_map::FileLoader + Send + Sync>>,
}

impl CompilerCallbacks {
    fn filesystem() -> Self {
        Self { file_loader: None }
    }

    fn database(loader: source_loader::SourceFileLoader) -> Self {
        Self {
            file_loader: Some(Box::new(loader)),
        }
    }
}

impl rustc_driver_impl::Callbacks for CompilerCallbacks {
    fn config(&mut self, config: &mut rustc_interface::Config) {
        println!("TRUEOS rustc min: compiler config entered");
        config.file_loader = self.file_loader.take();
        config.make_codegen_backend = Some(Box::new(|_| {
            rustc_codegen_cranelift::__rustc_codegen_backend()
        }));
        config.psess_created = Some(Box::new(|_| {
            println!("TRUEOS rustc min: compiler session created");
        }));
        config.track_state = Some(Box::new(|_| {
            println!("TRUEOS rustc min: compiler session state tracked");
        }));
        config.register_lints = Some(Box::new(|_, _| {
            println!("TRUEOS rustc min: compiler lint store created");
        }));
    }

    fn after_crate_root_parsing(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        _krate: &mut rustc_ast::ast::Crate,
    ) -> rustc_driver_impl::Compilation {
        println!("TRUEOS rustc min: crate root parsed");
        rustc_driver_impl::Compilation::Continue
    }

    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        _tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_driver_impl::Compilation {
        println!("TRUEOS rustc min: expansion complete");
        rustc_driver_impl::Compilation::Continue
    }

    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        _tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_driver_impl::Compilation {
        println!("TRUEOS rustc min: analysis complete");
        rustc_driver_impl::Compilation::Continue
    }
}

/// Copies the loader-owned C argv while execution is still on the Hull carrier.
///
/// The Blueprint loader guarantees that every non-null argv entry points to a
/// NUL-terminated byte string for the duration of `_start`. The explicit bounds
/// keep a malformed loader contract from causing an unbounded scan.
unsafe fn capture_compiler_argv(
    argc: usize,
    argv: *const *const c_char,
) -> Result<Vec<String>, String> {
    if argc == 0 || argv.is_null() {
        return Ok(vec![ARGV0.to_owned()]);
    }
    if argc > MAX_COMPILER_ARGS {
        return Err(format!(
            "loader supplied {argc} arguments; limit is {MAX_COMPILER_ARGS}"
        ));
    }

    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let arg = unsafe { *argv.add(index) };
        if arg.is_null() {
            return Err(format!("loader argv[{index}] is null"));
        }

        let mut len = 0usize;
        while len < MAX_COMPILER_ARG_BYTES && unsafe { *arg.add(len) } != 0 {
            len += 1;
        }
        if len == MAX_COMPILER_ARG_BYTES {
            return Err(format!(
                "loader argv[{index}] exceeds {MAX_COMPILER_ARG_BYTES} bytes"
            ));
        }

        let bytes = unsafe { core::slice::from_raw_parts(arg.cast::<u8>(), len) };
        args.push(String::from_utf8_lossy(bytes).into_owned());
    }

    if let Some(argv0) = args.first_mut() {
        if argv0.is_empty() {
            *argv0 = ARGV0.to_owned();
        }
    } else {
        args.push(ARGV0.to_owned());
    }
    Ok(args)
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|arg| arg == flag || arg.starts_with(&format!("{flag}=")))
}

fn extern_crate_name(value: &str) -> &str {
    value
        .split_once('=')
        .map_or(value, |(name, _)| name)
        .rsplit(':')
        .next()
        .unwrap_or(value)
}

fn has_extern_crate(args: &[String], crate_name: &str) -> bool {
    args.iter().enumerate().any(|(index, arg)| {
        if let Some(value) = arg.strip_prefix("--extern=") {
            extern_crate_name(value) == crate_name
        } else if arg == "--extern" {
            args.get(index + 1)
                .is_some_and(|value| extern_crate_name(value) == crate_name)
        } else {
            false
        }
    })
}

#[derive(Clone)]
struct EmbeddedMetadata {
    crate_name: String,
    file_name: String,
}

struct EmbeddedDirectExtern {
    alias: String,
    crate_name: String,
    file_name: String,
}

struct EmbeddedManifest {
    sysroot: Vec<EmbeddedMetadata>,
    payload: Vec<EmbeddedMetadata>,
    direct_externs: Vec<EmbeddedDirectExtern>,
}

fn valid_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn validate_metadata_file(crate_name: &str, file_name: &str) -> Result<(), String> {
    if !valid_crate_name(crate_name)
        || file_name.contains('/')
        || file_name.contains('\\')
        || !file_name.starts_with(&format!("lib{crate_name}-"))
        || !file_name.ends_with(".rmeta")
    {
        return Err(format!(
            "unsafe metadata entry in {SYSROOT_MANIFEST}: {crate_name} {file_name}"
        ));
    }
    Ok(())
}

fn manifest_count(value: &str, key: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("bad {key} in {SYSROOT_MANIFEST}: {value}"))
}

impl EmbeddedManifest {
    fn load() -> Result<Self, String> {
        let raw = fs::read_to_string(SYSROOT_MANIFEST)
            .map_err(|err| format!("read embedded sysroot manifest {SYSROOT_MANIFEST}: {err}"))?;
        let mut format = None;
        let mut target = None;
        let mut metadata_count = None;
        let mut payload_metadata_count = None;
        let mut payload_extern_count = None;
        let mut sysroot = Vec::new();
        let mut payload = Vec::new();
        let mut direct_externs = Vec::new();
        let mut rows = BTreeSet::new();

        for line in raw.lines() {
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("malformed line in {SYSROOT_MANIFEST}: {line}"));
            };
            match key {
                "format" => format = Some(value),
                "target" => target = Some(value),
                "metadata-count" => metadata_count = Some(manifest_count(value, key)?),
                "payload-metadata-count" => {
                    payload_metadata_count = Some(manifest_count(value, key)?)
                }
                "payload-extern-count" => payload_extern_count = Some(manifest_count(value, key)?),
                "metadata" | "payload" => {
                    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
                    if fields.len() != 2 {
                        return Err(format!("malformed {key} entry in {SYSROOT_MANIFEST}"));
                    }
                    validate_metadata_file(fields[0], fields[1])?;
                    if !rows.insert((key, fields[0], fields[1])) {
                        return Err(format!("duplicate {key} entry in {SYSROOT_MANIFEST}"));
                    }
                    let entry = EmbeddedMetadata {
                        crate_name: fields[0].to_owned(),
                        file_name: fields[1].to_owned(),
                    };
                    if key == "metadata" {
                        sysroot.push(entry);
                    } else {
                        payload.push(entry);
                    }
                }
                "extern" => {
                    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
                    if fields.len() != 3
                        || !valid_crate_name(fields[0])
                        || !valid_crate_name(fields[1])
                    {
                        return Err(format!("malformed extern entry in {SYSROOT_MANIFEST}"));
                    }
                    validate_metadata_file(fields[1], fields[2])?;
                    if !rows.insert((key, fields[0], fields[2])) {
                        return Err(format!("duplicate extern entry in {SYSROOT_MANIFEST}"));
                    }
                    direct_externs.push(EmbeddedDirectExtern {
                        alias: fields[0].to_owned(),
                        crate_name: fields[1].to_owned(),
                        file_name: fields[2].to_owned(),
                    });
                }
                _ => {}
            }
        }

        if format != Some("trueos-rustc-sysroot-v1") {
            return Err(format!("unsupported format in {SYSROOT_MANIFEST}"));
        }
        if target != Some(TARGET_TRIPLE) {
            return Err(format!("wrong target in {SYSROOT_MANIFEST}"));
        }
        if metadata_count != Some(sysroot.len())
            || payload_metadata_count != Some(payload.len())
            || payload_extern_count != Some(direct_externs.len())
        {
            return Err(format!("metadata count mismatch in {SYSROOT_MANIFEST}"));
        }
        for direct in &direct_externs {
            if !payload.iter().any(|artifact| {
                artifact.crate_name == direct.crate_name && artifact.file_name == direct.file_name
            }) {
                return Err(format!(
                    "direct extern {} has no payload artifact in {SYSROOT_MANIFEST}",
                    direct.alias
                ));
            }
        }

        Ok(Self {
            sysroot,
            payload,
            direct_externs,
        })
    }

    fn sysroot_path(&self, crate_name: &str) -> Result<String, String> {
        let mut matches = self
            .sysroot
            .iter()
            .filter(|metadata| metadata.crate_name == crate_name);
        let metadata = matches.next().ok_or_else(|| {
            format!("embedded sysroot manifest contains no metadata for {crate_name}")
        })?;
        if matches.next().is_some() {
            return Err(format!(
                "embedded sysroot manifest contains ambiguous metadata for {crate_name}"
            ));
        }
        Ok(format!(
            "{SYSROOT}/lib/rustlib/{TARGET_TRIPLE}/lib/{}",
            metadata.file_name
        ))
    }
}

fn checked_metadata_file(path: &str, label: &str) -> Result<u64, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("inspect embedded {label} metadata {path}: {err}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "embedded {label} metadata is not a non-empty file: {path}"
        ));
    }
    Ok(metadata.len())
}

fn persist_managed_local_source(source: &str) -> Result<usize, String> {
    fs::write(LOCAL_SOURCE, source.as_bytes())
        .map_err(|err| format!("write managed local compile source {LOCAL_SOURCE}: {err}"))?;
    let persisted = fs::read(LOCAL_SOURCE)
        .map_err(|err| format!("verify managed local compile source {LOCAL_SOURCE}: {err}"))?;
    if persisted.as_slice() != source.as_bytes() {
        return Err(format!(
            "managed local compile source readback mismatch: {LOCAL_SOURCE}"
        ));
    }
    Ok(persisted.len())
}

fn managed_gridpaper_source() -> String {
    GRIDPAPER_APP_SOURCE.to_owned()
}

fn is_managed_gridpaper_v1(source: &[u8]) -> bool {
    source.len() == 3302
        && core::str::from_utf8(source).is_ok_and(|source| {
            source.starts_with("#![no_std]\n#![no_main]\n")
                && source.contains("let mut page = [0_u8; PAGE_BYTES];")
                && source.contains("fn draw_rustc_gridpaper(")
                && source.contains("RUSTC ONE-SHOT GRIDPAPER")
                && source.contains("compiled inside TRUEOS")
                && source.contains("rustc one-shot: Gridpaper snapshot submitted")
        })
}

fn is_managed_gridpaper_v2(source: &[u8]) -> bool {
    source.len() == 3361
        && core::str::from_utf8(source).is_ok_and(|source| {
            source.starts_with("#![no_std]\n#![no_main]\n")
                && source.contains("static mut PAGE: [u8; PAGE_BYTES]")
                && source.contains("let page = unsafe { &mut *(&raw mut PAGE) };")
                && source.contains("colors[index % colors.len()]")
                && source.contains("RUSTC ONE-SHOT GRIDPAPER")
                && source.contains("compiled inside TRUEOS")
                && source.contains("rustc one-shot: Gridpaper snapshot submitted")
        })
}

fn is_managed_gridpaper_v3(source: &[u8]) -> bool {
    source.len() == 3505
        && core::str::from_utf8(source).is_ok_and(|source| {
            source.starts_with("#![no_std]\n#![no_main]\n")
                && source.contains("static mut PAGE: [u8; PAGE_BYTES]")
                && source.contains("let page = (&raw mut PAGE).cast::<u8>();")
                && source.contains("*colors.as_ptr().add(index % colors.len())")
                && source.contains("RUSTC ONE-SHOT GRIDPAPER")
                && source.contains("compiled inside TRUEOS")
                && source.contains("rustc one-shot: Gridpaper snapshot submitted")
        })
}

fn prepare_local_source() -> Result<(), String> {
    let directory = Path::new(LOCAL_COMPILE_DIR);
    fs::create_dir_all(directory)
        .map_err(|err| format!("create local compile directory {LOCAL_COMPILE_DIR}: {err}"))?;
    let managed_gridpaper = managed_gridpaper_source();

    match fs::metadata(LOCAL_SOURCE) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(format!(
                    "local compile source is not a file: {LOCAL_SOURCE}"
                ));
            }
            if metadata.len() == 0 || metadata.len() > MAX_LOCAL_SOURCE_BYTES {
                return Err(format!(
                    "local compile source size is outside 1..={MAX_LOCAL_SOURCE_BYTES}: path={LOCAL_SOURCE} bytes={}",
                    metadata.len()
                ));
            }
            let existing = fs::read(LOCAL_SOURCE)
                .map_err(|err| format!("read local compile source {LOCAL_SOURCE}: {err}"))?;
            if existing.is_empty() || existing.len() as u64 > MAX_LOCAL_SOURCE_BYTES {
                return Err(format!(
                    "local compile source changed outside 1..={MAX_LOCAL_SOURCE_BYTES} bytes while reading: path={LOCAL_SOURCE} bytes={}",
                    existing.len()
                ));
            }
            let previous_managed_seed = if existing.as_slice() == managed_gridpaper.as_bytes() {
                None
            } else if existing.as_slice() == MANAGED_LOCAL_SOURCE_V1.as_bytes() {
                Some("v1")
            } else if existing.as_slice() == MANAGED_LOCAL_SOURCE_V2.as_bytes() {
                Some("v2")
            } else if existing.as_slice() == MANAGED_LOCAL_SOURCE_V3.as_bytes() {
                Some("v3")
            } else if existing.as_slice() == MANAGED_LOCAL_SOURCE_V4.as_bytes() {
                Some("v4")
            } else if is_managed_gridpaper_v1(existing.as_slice()) {
                Some("gridpaper-v1")
            } else if is_managed_gridpaper_v2(existing.as_slice()) {
                Some("gridpaper-v2")
            } else if is_managed_gridpaper_v3(existing.as_slice()) {
                Some("gridpaper-v3")
            } else {
                None
            };
            if let Some(previous_managed_seed) = previous_managed_seed {
                let bytes = persist_managed_local_source(&managed_gridpaper)?;
                println!(
                    "TRUEOS rustc min: local source migrated path={LOCAL_SOURCE} managed_seed={previous_managed_seed}->gridpaper-v4 bytes={bytes}"
                );
            } else {
                let source_kind = if existing.as_slice() == managed_gridpaper.as_bytes() {
                    "managed_seed=gridpaper-v4"
                } else {
                    "user_owned=1"
                };
                println!(
                    "TRUEOS rustc min: local source existing path={LOCAL_SOURCE} bytes={} {source_kind}",
                    existing.len()
                );
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let bytes = persist_managed_local_source(&managed_gridpaper)?;
            println!(
                "TRUEOS rustc min: local source seeded path={LOCAL_SOURCE} managed_seed=gridpaper-v4 bytes={bytes}"
            );
        }
        Err(err) => {
            return Err(format!(
                "inspect local compile source {LOCAL_SOURCE}: {err}"
            ));
        }
    }
    Ok(())
}

fn compiler_args(
    mut args: Vec<String>,
    use_embedded_dependencies: bool,
) -> Result<Vec<String>, String> {
    let local_compile = args.len() == 1;
    if local_compile {
        args.extend([
            LOCAL_SOURCE.into(),
            "--crate-name".into(),
            "trueos_local".into(),
            "--edition=2024".into(),
            "--crate-type".into(),
            "lib".into(),
            "--emit=obj".into(),
            "-Cpanic=abort".into(),
            "-Cdebuginfo=0".into(),
            "-Copt-level=1".into(),
            "-Ccodegen-units=1".into(),
            "-Zno-parallel-backend".into(),
            "-o".into(),
            LOCAL_OBJECT.into(),
        ]);
    }
    if use_embedded_dependencies {
        let manifest = EmbeddedManifest::load()?;
        for crate_name in ["core", "compiler_builtins", "alloc"] {
            if has_extern_crate(&args, crate_name) {
                continue;
            }
            let metadata = manifest.sysroot_path(crate_name)?;
            checked_metadata_file(&metadata, crate_name)?;
            println!("TRUEOS rustc min: {crate_name} metadata {metadata}");
            args.extend(["--extern".into(), format!("{crate_name}={metadata}")]);
        }
        // Keep command-line externs limited to roots used by the source.
        // `--sysroot` supplies the authenticated standard-library closure and
        // this dependency path supplies the embedded application/API closure.
        // Passing every transitive artifact as a command-line extern activates
        // rustc's exact-path handling for duplicate crate names and defeats
        // normal hashed dependency selection.
        args.extend(["-L".into(), format!("dependency={PAYLOAD_LIB}")]);
        for direct in &manifest.direct_externs {
            let path = format!("{PAYLOAD_LIB}/{}", direct.file_name);
            let bytes = checked_metadata_file(&path, direct.alias.as_str())?;
            println!(
                "TRUEOS rustc min: direct extern {}={} bytes={bytes}",
                direct.alias, path
            );
            args.extend(["--extern".into(), format!("{}={path}", direct.alias)]);
        }
        if !manifest.sysroot.is_empty() || !manifest.payload.is_empty() {
            println!(
                "TRUEOS rustc min: sysroot metadata={} payload metadata={} direct={}",
                manifest.sysroot.len(),
                manifest.payload.len(),
                manifest.direct_externs.len()
            );
        }
    }
    if !has_flag(&args, "--target") {
        args.extend(["--target".into(), TARGET.into()]);
    }
    if !has_flag(&args, "--sysroot") {
        args.extend(["--sysroot".into(), SYSROOT.into()]);
    }
    if !has_flag(&args, "-Zunstable-options") {
        args.push("-Zunstable-options".into());
    }
    Ok(args)
}

fn compile(
    args: Vec<String>,
    local_compile: bool,
) -> Result<Option<blueprint::PublishedBlueprint>, String> {
    if local_compile {
        // Keep the entire `/common` handoff on the compiler carrier. Hull
        // synchronous I/O crosses VMCALL before reaching the same filesystem
        // broker and would otherwise resolve the application path twice.
        prepare_local_source()?;
        println!(
            "TRUEOS rustc min: local compile contract=v15 realm=compiler-carrier seed=gridpaper-v4 storage=static-bss cg-context=fresh-per-function core-panic=bounds-check-portal opt-level=1 extern-mode=roots+search api=trueos::gridpaper source={LOCAL_SOURCE} object={LOCAL_OBJECT} output={}",
            blueprint::OUTPUT_PATH
        );
    }
    println!("TRUEOS rustc min: compiler entered");
    let exit = rustc_driver_impl::catch_with_exit_code(|| {
        rustc_driver_impl::run_compiler(&args, &mut CompilerCallbacks::filesystem())
    });
    if exit != ExitCode::SUCCESS {
        return Err(format!("rustc exited with {exit:?}"));
    }
    if local_compile {
        blueprint::publish(Path::new(LOCAL_OBJECT)).map(Some)
    } else {
        Ok(None)
    }
}

struct DatabaseCompileResult {
    published: blueprint_object::PublishedBlueprint,
    output: String,
    project: String,
    revision: u64,
}

fn compile_database(
    request: source_commands::CompileRequest,
) -> Result<DatabaseCompileResult, String> {
    let source_commands::CompileRequest {
        source,
        entry,
        output,
    } = request;
    let revision = source.revision.revision;
    let project = source.revision.project.clone();
    let revision_i64 =
        i64::try_from(revision).map_err(|_| String::from("database revision exceeds i64"))?;
    let loader = source_loader::SourceFileLoader::new(
        project.as_str(),
        revision_i64,
        source.files.into_iter().map(|file| (file.path, file.bytes)),
    )?;
    let entry_path = loader.entry_path(entry.as_deref())?;
    let artifact = safe_artifact_name(project.as_str());
    let crate_name = artifact.replace('-', "_");
    let output = output.unwrap_or_else(|| format!("{LOCAL_COMPILE_DIR}/{artifact}.bp"));
    if Path::new(output.as_str())
        .extension()
        .and_then(|value| value.to_str())
        != Some("bp")
    {
        return Err(format!("Blueprint output must end in `.bp`: {output}"));
    }
    let compile_id = COMPILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let started = trueos::clock::monotonic_nanos();
    let object = format!("rustc-build/{artifact}-r{revision}-{started:016x}-{compile_id:016x}.o");
    let object_path = Path::new(object.as_str());
    if let Some(parent) = object_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create compiler object directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let args = compiler_args(
        vec![
            ARGV0.into(),
            entry_path.to_string_lossy().into_owned(),
            "--crate-name".into(),
            crate_name,
            "--edition=2024".into(),
            "--crate-type".into(),
            "lib".into(),
            "--emit=obj".into(),
            "-Cpanic=abort".into(),
            "-Cdebuginfo=0".into(),
            "-Copt-level=1".into(),
            "-Ccodegen-units=1".into(),
            "-Zno-parallel-backend".into(),
            "-o".into(),
            object.clone(),
        ],
        true,
    )?;
    println!(
        "TRUEOS rustc min: database compile project={project} revision={revision} files={} source-bytes={} entry={} object={} output={output}",
        loader.file_count(),
        source.revision.source_bytes,
        entry_path.display(),
        object
    );

    let result = (|| {
        let mut callbacks = CompilerCallbacks::database(loader);
        let exit = rustc_driver_impl::catch_with_exit_code(|| {
            rustc_driver_impl::run_compiler(&args, &mut callbacks)
        });
        if exit != ExitCode::SUCCESS {
            return Err(format!("rustc exited with {exit:?}"));
        }
        blueprint_object::publish(object_path, Path::new(output.as_str()))
    })();
    let cleanup = match fs::remove_file(object_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "remove transient compiler object {}: {error}",
            object_path.display()
        )),
    };
    let published = match (result, cleanup) {
        (Ok(published), Ok(())) => published,
        (Err(error), _) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
    };
    Ok(DatabaseCompileResult {
        published,
        output,
        project,
        revision,
    })
}

fn safe_artifact_name(project: &str) -> String {
    let mut output = String::new();
    for character in project.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            output.push(character);
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        String::from("app")
    } else {
        output.to_owned()
    }
}

fn run(process_args: Vec<String>) -> bool {
    println!("TRUEOS rustc {TIER}: nightly-2026-07-10 af3d95584 workers={WORKERS}");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build TRUEOS Tokio runtime");

    if source_commands::is_source_command(process_args.as_slice()) {
        let outcome = runtime.block_on(source_commands::execute(process_args.as_slice()));
        return match outcome {
            Ok(source_commands::Outcome::Complete) => true,
            Ok(source_commands::Outcome::Compile(request)) => {
                let result = runtime.block_on(async move {
                    tokio::task::spawn_blocking(move || compile_database(request))
                        .await
                        .map_err(|error| format!("compiler lane failed: {error}"))?
                });
                match result {
                    Ok(compiled) => {
                        println!(
                            "TRUEOS rustc {TIER}: database Blueprint compile passed project={} revision={} path={} bytes={} object={} imports={}",
                            compiled.project,
                            compiled.revision,
                            compiled.output,
                            compiled.published.blueprint_bytes,
                            compiled.published.object_bytes,
                            compiled.published.imports.len()
                        );
                        true
                    }
                    Err(error) => {
                        eprintln!("TRUEOS rustc {TIER}: {error}");
                        false
                    }
                }
            }
            Err(error) => {
                eprintln!("TRUEOS rustc {TIER}: {error}");
                false
            }
        };
    }

    let local_compile = process_args.len() == 1;
    let args = match compiler_args(process_args, local_compile) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("TRUEOS rustc {TIER}: {err}");
            return false;
        }
    };
    let result = runtime.block_on(async move {
        tokio::task::spawn_blocking(move || compile(args, local_compile))
            .await
            .map_err(|err| format!("compiler lane failed: {err}"))?
    });
    match result {
        Ok(Some(published)) => {
            println!("TRUEOS rustc {TIER}: file-backed local Blueprint compile passed");
            println!(
                "TRUEOS rustc {TIER}: Blueprint stored path={} bytes={} object={} entry=sec:{}+0x{:x}",
                blueprint::OUTPUT_PATH,
                published.blueprint_bytes,
                published.object_bytes,
                published.entry_hint >> 32,
                published.entry_hint as u32,
            );
            true
        }
        Ok(None) => {
            println!("TRUEOS rustc {TIER}: compiler invocation passed");
            true
        }
        Err(err) => {
            eprintln!("TRUEOS rustc {TIER}: {err}");
            false
        }
    }
}

#[unsafe(no_mangle)]
#[cfg(not(test))]
pub extern "C" fn _start(argc: usize, argv: *const *const c_char) -> i32 {
    let process_args = match unsafe { capture_compiler_argv(argc, argv) } {
        Ok(args) => args,
        Err(err) => {
            eprintln!("TRUEOS rustc {TIER}: {err}");
            return 1;
        }
    };
    if run(process_args) { 0 } else { 1 }
}

// The host unit-test binary does not run under the Blueprint loader, so supply
// only the two console/lifecycle imports retained by the test link.
#[cfg(test)]
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

#[cfg(test)]
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_blueprint_shutdown(_data: *const u8, _len: usize) -> i32 {
    0
}
