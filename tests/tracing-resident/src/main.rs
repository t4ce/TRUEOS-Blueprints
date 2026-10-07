use std::{
    io::Write,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Mutex,
    },
};
static RECORDS: Mutex<Vec<(u32, String, String)>> = Mutex::new(Vec::new());
static LEVEL: AtomicU32 = AtomicU32::new(5);
static IDS: AtomicUsize = AtomicUsize::new(1);
thread_local! { static ID: usize = IDS.fetch_add(1, Ordering::Relaxed); }
#[no_mangle]
extern "C" fn trueos_cabi_thread_current_id() -> usize {
    ID.with(|id| *id)
}
#[no_mangle]
unsafe extern "C" fn trueos_cabi_log(
    level: u32,
    target: *const u8,
    target_len: usize,
    msg: *const u8,
    len: usize,
) -> i32 {
    if level & (1 << 31) != 0 {
        return ((level & !(1 << 31)) <= LEVEL.load(Ordering::Relaxed)) as i32;
    }
    if level > LEVEL.load(Ordering::Relaxed) {
        return 0;
    }
    let target = std::str::from_utf8(std::slice::from_raw_parts(target, target_len))
        .unwrap()
        .to_owned();
    let message = std::str::from_utf8(std::slice::from_raw_parts(msg, len))
        .unwrap()
        .to_owned();
    assert!(message.len() <= 1024 && !message.chars().any(char::is_control));
    RECORDS.lock().unwrap().push((level, target, message));
    len as i32
}
struct Forbidden;
impl Write for Forbidden {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        panic!("app writer was used")
    }
    fn flush(&mut self) -> std::io::Result<()> {
        panic!("app writer was flushed")
    }
}
impl std::fmt::Debug for Forbidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        tracing::info!("recursive event");
        f.write_str("safe")
    }
}
#[tracing::instrument(target = "resident", skip_all)]
fn instrumented() {
    tracing::info!(target: "resident", "attribute event");
}

fn main() {
    instrumented();
    tracing::subscriber::with_default(
        tracing_subscriber::registry(),
        || tracing::info!(target: "resident", "direct dispatch"),
    );
    tracing::info!(target:"resident", answer=42, "before initialization");
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::fmt()
        .with_env_filter("off")
        .with_writer(|| Forbidden)
        .finish()
        .try_init()
        .unwrap();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .finish()
        .try_init()
        .unwrap();
    tracing::error!(target:"resident", "error");
    tracing::warn!(target:"resident", "warn");
    tracing::debug!(target:"resident", "debug");
    tracing::trace!(target:"resident", "trace");
    LEVEL.store(2, Ordering::Relaxed);
    let evaluated = AtomicUsize::new(0);
    tracing::info!(target:"resident", value=evaluated.fetch_add(1,Ordering::Relaxed), "disabled");
    assert_eq!(evaluated.load(Ordering::Relaxed), 0);
    LEVEL.store(5, Ordering::Relaxed);
    let parent = tracing::info_span!(target:"resident", "parent", user="t4ce");
    let child = tracing::info_span!(target:"resident",parent:&parent,"child", revision=tracing::field::Empty);
    child.follows_from(&parent);
    drop(parent);
    let entered = child.enter();
    child.record("revision", 7);
    assert_eq!(tracing::Span::current().id(), child.id());
    tracing::info!(target:"resident", value=?Forbidden, "nested");
    std::thread::spawn(|| tracing::info!(target:"resident", "isolated"))
        .join()
        .unwrap();
    drop(entered);
    for _ in 0..4 {
        child.record("revision", "é".repeat(1000));
    }
    tracing::info!(target:"resident",parent:&child, "large {}", "é".repeat(3000));
    drop(child);
    let (mut writer, guard) = tracing_appender::non_blocking(Forbidden);
    writer.write_all(b"appender\nrecord").unwrap();
    writer.flush().unwrap();
    drop(guard);
    let path = std::env::temp_dir().join(format!("trueos-no-files-{}", std::process::id()));
    assert!(!path.exists());
    let mut rolling = tracing_appender::rolling::weekly(&path, "test.log");
    rolling.write_all(b"rolling").unwrap();
    assert!(!path.exists());
    log::info!(target:"legacy", "legacy bridge");
    let records = RECORDS.lock().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.2.contains("before initialization"))
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.2.contains("legacy bridge"))
            .count(),
        1
    );
    assert!(!records
        .iter()
        .any(|r| r.2.contains("recursive event") || r.2.contains("disabled")));
    let nested = records.iter().find(|r| r.2.contains("nested")).unwrap();
    assert!(
        nested.2.contains("parent")
            && nested.2.contains("user=\"t4ce\"")
            && nested.2.contains("revision=7")
    );
    assert!(!records
        .iter()
        .find(|r| r.2.contains("isolated"))
        .unwrap()
        .2
        .contains("span="));
    for level in 1..=5 {
        assert!(records.iter().any(|r| r.0 == level));
    }
    println!("resident transport passed: {} bounded records; filtering, spans, recursion, isolation, legacy bridge, no file/worker sink",records.len());
}
