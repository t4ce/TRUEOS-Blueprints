//! Exercise libc/rustix rename through real tempfile persist with live File FDs.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};

fn record(start: Instant, state: &str, case: &str, detail: &str) {
    let line = format!("file_replace_contract: {state} case={case} elapsed_ms={} {detail}", start.elapsed().as_millis());
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, format_args!("{line}"));
    #[cfg(not(target_os = "trueos"))]
    println!("{line}");
}

fn require(condition: bool, detail: &'static str) -> io::Result<()> {
    if condition { Ok(()) } else { Err(io::Error::other(detail)) }
}

fn replace_contents(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn read_handle(file: &mut File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn open_existing(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

fn save(path: &Path, bytes: &[u8]) -> io::Result<File> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(bytes)?;
    temporary.persist(path).map_err(|error| error.error)
}

fn persist_with_live_source(root: &Path) -> io::Result<()> {
    let destination = root.join("source-live-config.toml");
    fs::write(&destination, b"generation=old\n")?;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    temporary.write_all(b"generation=one\n")?;
    let source_path = temporary.path().to_owned();
    let mut retained_source = temporary.as_file().try_clone()?;
    let mut persisted = temporary.persist(&destination).map_err(|error| error.error)?;
    require(!source_path.try_exists()?, "persist left source pathname")?;
    require(fs::read(&destination)? == b"generation=one\n", "persist did not replace existing destination")?;
    replace_contents(&mut retained_source, b"generation=source-fd\n")?;
    require(fs::read(&destination)? == b"generation=source-fd\n", "retained source fd did not update destination")?;
    require(!source_path.try_exists()?, "source fd write resurrected temporary pathname")?;
    replace_contents(&mut persisted, b"generation=persisted-fd\n")?;
    require(fs::read(&destination)? == b"generation=persisted-fd\n", "returned persist fd did not update destination")?;
    drop(retained_source);
    drop(persisted);
    require(!source_path.try_exists()?, "closing source fd resurrected temporary pathname")?;
    require(fs::read(&destination)? == b"generation=persisted-fd\n", "close lost persisted content")
}

fn detached_destination(root: &Path) -> io::Result<()> {
    let destination = root.join("destination-live-config.toml");
    fs::write(&destination, b"old destination bytes\n")?;
    let mut old_destination = open_existing(&destination)?;
    let replacement = save(&destination, b"replacement bytes\n")?;
    require(read_handle(&mut old_destination)? == b"old destination bytes\n", "open destination fd lost old object on replacement")?;
    replace_contents(&mut old_destination, b"detached old object writes\n")?;
    require(read_handle(&mut old_destination)? == b"detached old object writes\n", "detached fd lost its writes")?;
    require(fs::read(&destination)? == b"replacement bytes\n", "old destination fd clobbered replacement")?;
    drop(old_destination);
    drop(replacement);
    require(fs::read(&destination)? == b"replacement bytes\n", "closing old destination clobbered replacement")
}

fn failed_rename_preserves_both(root: &Path) -> io::Result<()> {
    let destination = root.join("nonempty-directory");
    fs::create_dir(&destination)?;
    let child = destination.join("keep.txt");
    fs::write(&child, b"target survives\n")?;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    temporary.write_all(b"source survives\n")?;
    temporary.as_file().sync_all()?;
    let source_path = temporary.path().to_owned();
    let failure = match temporary.persist(&destination) {
        Ok(_) => return Err(io::Error::other("file persist over nonempty directory unexpectedly succeeded")),
        Err(failure) => failure,
    };
    require(source_path.try_exists()?, "failed rename removed source pathname")?;
    require(fs::read(&source_path)? == b"source survives\n", "failed rename changed source")?;
    require(fs::read(&child)? == b"target survives\n", "failed rename changed destination")?;
    require(read_handle(&mut failure.file.as_file().try_clone()?)? == b"source survives\n", "failed persist lost source fd")?;
    // Dropping the recovered NamedTempFile cleans only this probe-owned source.
    drop(failure);
    require(!source_path.try_exists()?, "failed persist source cleanup failed")?;
    require(fs::read(&child)? == b"target survives\n", "source cleanup changed target")
}

fn repeated_save(root: &Path) -> io::Result<()> {
    let destination = root.join("repeated-config.toml");
    fs::write(&destination, b"generation=zero\n")?;
    let mut first = save(&destination, b"generation=one\n")?;
    let mut second = save(&destination, b"generation=two\n")?;
    require(fs::read(&destination)? == b"generation=two\n", "second save did not replace first")?;
    require(read_handle(&mut first)? == b"generation=one\n", "second save changed first open file")?;
    replace_contents(&mut first, b"generation=detached-first\n")?;
    require(fs::read(&destination)? == b"generation=two\n", "first save fd clobbered second save")?;
    replace_contents(&mut second, b"generation=two-updated\n")?;
    drop(first);
    drop(second);
    require(fs::read(&destination)? == b"generation=two-updated\n", "second source fd writes did not persist")
}

fn main() {
    let started = Instant::now();
    // Bound blocked filesystem operations too; the watchdog must not join the
    // worker or wait for a syscall to return before reporting a timed-out run.
    let (watchdog_stop, watchdog_wait) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if matches!(watchdog_wait.recv_timeout(Duration::from_secs(120)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
            record(started, "FAIL", "suite.timeout", "deadline_ms=120000");
            std::process::exit(124);
        }
    });
    record(started, "BEGIN", "suite", "backend=tempfile-3.27-rustix protocol=1 deadline_ms=120000");
    let root = match tempfile::Builder::new().prefix("file-replace-contract-").tempdir() {
        Ok(root) => root,
        Err(error) => {
            record(started, "FAIL", "suite.setup", &error.to_string());
            std::process::exit(1);
        }
    };
    let cases: [(&str, fn(&Path) -> io::Result<()>); 4] = [
        ("persist.open_source_fd", persist_with_live_source),
        ("persist.detached_destination_fd", detached_destination),
        ("persist.failed_rename_preserves_both", failed_rename_preserves_both),
        ("persist.second_save", repeated_save),
    ];
    let (mut passed, mut failed) = (0, 0);
    for (case, run) in cases {
        record(started, "BEGIN", case, "");
        match run(root.path()) {
            Ok(()) => { passed += 1; record(started, "PASS", case, ""); }
            Err(error) => { failed += 1; record(started, "FAIL", case, &error.to_string()); }
        }
    }
    match root.close() {
        Ok(()) => { passed += 1; record(started, "PASS", "cleanup.owned_directory", ""); }
        Err(error) => { failed += 1; record(started, "FAIL", "cleanup.owned_directory", &error.to_string()); }
    }
    let stop_sent = watchdog_stop.send(()).is_ok();
    let joined = watchdog.join().is_ok();
    if !stop_sent || !joined {
        failed += 1;
        record(started, "FAIL", "watchdog.teardown", "stop_and_join_failed");
    }
    record(started, if failed == 0 { "PASS" } else { "FAIL" }, "suite", &format!("passed={passed} failed={failed}"));
    if failed != 0 { std::process::exit(1); }
}
