#!/usr/bin/env python3
"""Exercise Veloren's native source and real TRUEOS async client over a mock CABI.

The mock returns the kernel's actual binary directory encoding, pending jobs,
and short result chunks. Run from any directory beside the TRUEOS checkouts.
"""
import os
from pathlib import Path
import subprocess
import tempfile

VELOREN = Path(__file__).resolve().parents[3]
BLUEPRINTS = VELOREN.parent.parent
KERNEL = Path(os.environ.get("TRUEOS_REPO", BLUEPRINTS.parent / "TRUEOS"))

MOCK = r'''
use std::{collections::BTreeMap, sync::{Mutex, atomic::{AtomicU32, Ordering}}};
struct Operation { data: Vec<u8>, pending: bool }
static OPS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);
fn admit(data: std::io::Result<Vec<u8>>) -> i32 {
    match data {
        Ok(data) => {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            OPS.lock().unwrap().insert(id, Operation { data, pending: true });
            id as i32
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => -8,
        Err(_) => -2,
    }
}
unsafe fn path<'a>(p: *const u8, n: usize) -> &'a std::path::Path {
    std::path::Path::new(std::str::from_utf8(unsafe { std::slice::from_raw_parts(p, n) }).unwrap())
}
#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_async_fs_read_start(p: *const u8, n: usize) -> i32 {
    admit(std::fs::read(unsafe { path(p, n) }))
}
#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_async_fs_stat_start(p: *const u8, n: usize) -> i32 {
    admit(std::fs::metadata(unsafe { path(p, n) }).map(|m| {
        let kind: u32 = if m.is_dir() { 2 } else { 1 };
        [kind.to_le_bytes().as_slice(), m.len().to_le_bytes().as_slice()].concat()
    }))
}
mod r { pub mod fs { pub mod trueosfs {
    pub use trueos::async_fs::NodeKind;
    pub struct DirEntry { pub name: String, pub kind: NodeKind }
    pub struct DirListing { pub entries: Vec<DirEntry>, pub truncated: bool }
}}}
const DIR_LIST_MAGIC: [u8; 4] = *b"TDL1";
const DIR_LIST_HEADER_BYTES: usize = 12;
__ENCODER__
#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_async_fs_list_dir_start(p: *const u8, n: usize) -> i32 {
    let path = unsafe { path(p, n) };
    admit((|| {
        let mut entries = Vec::new();
        for e in std::fs::read_dir(path)? {
            let e = e?;
            entries.push(r::fs::trueosfs::DirEntry {
                name: e.file_name().into_string().unwrap(),
                kind: if e.file_type()?.is_dir() { r::fs::trueosfs::NodeKind::Directory }
                      else { r::fs::trueosfs::NodeKind::File },
            });
        }
        let listing = r::fs::trueosfs::DirListing {
            entries, truncated: path.ends_with("truncated-list"),
        };
        let bytes = if path.ends_with("invalid-list") { b"invalid\0listing".to_vec() }
                    else { encode_dir_listing(&listing).unwrap() };
        Ok(bytes)
    })())
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_async_fs_status(id: u32) -> i32 {
    let mut ops = OPS.lock().unwrap();
    let op = ops.get_mut(&id).unwrap();
    if op.pending { op.pending = false; 0 } else { 1 }
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_async_fs_result_len(id: u32) -> isize {
    OPS.lock().unwrap()[&id].data.len() as isize
}
#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_cabi_async_fs_result_read(id: u32, offset: usize, p: *mut u8, cap: usize) -> isize {
    let ops = OPS.lock().unwrap();
    let bytes = &ops[&id].data;
    // Stat is a fixed-size record; directory/file results exercise short reads.
    let chunk = if bytes.len() == 12 { cap } else { cap.min(7) };
    let n = chunk.min(bytes.len().saturating_sub(offset));
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().add(offset), p, n) };
    n as isize
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_async_fs_discard(id: u32) -> i32 {
    OPS.lock().unwrap().remove(&id).unwrap(); 0
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_poll_once() { std::thread::yield_now(); }
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 { std::process::abort(); }
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_write(_: u32, _: *const u8, _: usize) { std::process::abort(); }
'''

TESTS = r'''
#[test]
fn native_contract_errors_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    for folder in ["truncated-list", "invalid-list"] {
        let path = temp.path().join(folder);
        std::fs::create_dir(&path).unwrap();
        assert!(native::FileSystem::new(path).is_err());
    }
    let missing = native::FileSystem::new(temp.path().join("missing")).unwrap_err();
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    let file = temp.path().join("file");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(native::FileSystem::new(file).unwrap_err().kind(), std::io::ErrorKind::NotADirectory);
}
#[test]
fn native_source_preserves_ids_and_kinds() {
    use assets_manager::source::{Source, DirEntry};
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("entity")).unwrap();
    std::fs::write(temp.path().join("entity/unit.ron"), b"(1)").unwrap();
    std::fs::write(temp.path().join(".hidden"), b"ignore").unwrap();
    let fs = native::FileSystem::new(temp.path()).unwrap();
    let mut entries = Vec::new();
    fs.read_dir("", &mut |e| entries.push((e.id().to_owned(), e.is_dir()))).unwrap();
    assert_eq!(entries, vec![("entity".to_owned(), true)]);
    assert!(fs.exists(DirEntry::File("entity.unit", "ron")));
    assert!(!fs.exists(DirEntry::File("entity.missing", "ron")));
    assert_eq!(fs.read("entity.unit", "ron").unwrap().as_ref(), b"(1)");
}
#[test]
fn native_source_reads_real_server_files() {
    use assets_manager::source::Source;
    let fs = native::FileSystem::new(&*ASSETS_PATH).unwrap();
    for (id, ext) in [("common.canary", "canary"),
                       ("common.abilities.ability_set_manifest", "ron"),
                       ("server.manifests.kits", "ron"),
                       ("world.map.veloren_0_18_0_0", "bin")] {
        assert_eq!(fs.read(id, ext).unwrap().as_ref(), std::fs::read(fs.path_of(assets_manager::source::DirEntry::File(id, ext))).unwrap());
    }
}
#[test]
fn native_cache_startup_checks_canary() {
    use assets_manager::{AssetCache, source::Source};
    let source = fs::FileSystem::new().expect("initialize native asset cache source");
    assert!(source.read("common.canary", "canary").unwrap().as_ref().starts_with(b"VELOREN_CANARY_MAGIC"));
    let _cache = AssetCache::with_source(source);
}
'''


def main():
    encoder = (KERNEL / "src/r/io/async_fs_cabi.rs").read_text()
    start = encoder.index("fn encode_dir_listing(")
    encoder = encoder[start:encoder.index("\npub(crate) fn encode_typed_dir_listing", start)]
    with tempfile.TemporaryDirectory(prefix="veloren-trueos-assets-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        manifest = f'''[package]
name = "veloren-native-source-contract"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
trueos = {{ path = "{BLUEPRINTS / 'api'}", default-features = false }}
assets_manager = {{ version = "=0.13.9", features = ["ron", "json"] }}
hashbrown = {{ version = "0.17", features = ["serde"] }}
tracing = "0.1"
lazy_static = "1.4"
serde = {{ version = "=1.0.228", features = ["derive"] }}
tempfile = "3.27"
'''
        (root / "Cargo.toml").write_text(manifest)
        source_dir = VELOREN / "common/assets/src"
        wrapper = (source_dir / "fs.rs").read_text()
        # Enable only the TRUEOS source branch for this host transport test.
        wrapper = wrapper.replace('#[cfg(not(target_os = "trueos"))]', '#[cfg(any())]')
        wrapper = wrapper.replace('#[cfg(target_os = "trueos")]', '')
        wrapper = wrapper.replace('#[path = "trueos_fs.rs"]', f'#[path = "{source_dir / "trueos_fs.rs"}"]')
        (root / "src/fs.rs").write_text(wrapper)
        source = f'''pub use assets_manager::Asset;
use std::path::PathBuf;
lazy_static::lazy_static! {{ pub static ref ASSETS_PATH: PathBuf = PathBuf::from("{VELOREN / 'assets'}"); }}
mod fs;
#[path = "{source_dir / 'trueos_fs.rs'}"] mod native;
'''
        source += MOCK.replace("__ENCODER__", encoder) + TESTS
        (root / "src/lib.rs").write_text(source)
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = "/tmp/veloren-trueos-assets-contract-target"
        subprocess.run(["cargo", "+nightly-2026-07-10", "test", "--offline", "--manifest-path", str(root / "Cargo.toml")], cwd="/tmp", env=env, check=True)


if __name__ == "__main__":
    main()
