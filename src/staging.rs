//! Preserve Cargo's file timestamps while regenerating the private build tree.

use std::fs::{self, File, FileTimes};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Keep the previous staging tree until its replacement has been generated.
///
/// Source transformations can rewrite a manifest several times while staging.
/// Comparing the final bytes lets Cargo retain its usual timestamp-based
/// freshness checks without keeping removed or outdated staged files.
pub(crate) struct StagingGuard {
    work_dir: PathBuf,
    snapshot_dir: PathBuf,
    previous_tree: Option<PathBuf>,
}

impl StagingGuard {
    pub(crate) fn new(work_dir: &Path) -> Result<Self, String> {
        let parent = work_dir
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(io_string)?;
        let snapshot_dir = loop {
            let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".blueprint-stage-previous-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_string(error)),
            }
        };
        let mut guard = Self {
            work_dir: work_dir.to_path_buf(),
            snapshot_dir,
            previous_tree: None,
        };
        match fs::symlink_metadata(work_dir) {
            Ok(_) => {
                let previous = guard.snapshot_dir.join("tree");
                fs::rename(work_dir, &previous).map_err(io_string)?;
                guard.previous_tree = Some(previous);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_string(error)),
        }
        if let Err(error) = fs::create_dir_all(work_dir) {
            if let Some(previous) = &guard.previous_tree {
                // A failed setup should leave the old staging tree available.
                let _ = fs::rename(previous, work_dir);
            }
            return Err(io_string(error));
        }
        Ok(guard)
    }

    /// Restore modification times for identical files and complete subtrees.
    pub(crate) fn finish(self) -> Result<(), String> {
        if let Some(previous) = &self.previous_tree {
            restore_unchanged_times(previous, &self.work_dir).map_err(io_string)?;
        }
        fs::remove_dir_all(&self.snapshot_dir).map_err(io_string)
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.snapshot_dir);
    }
}

fn restore_unchanged_times(previous: &Path, current: &Path) -> io::Result<bool> {
    let old_metadata = match fs::symlink_metadata(previous) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let new_metadata = fs::symlink_metadata(current)?;
    if old_metadata.is_dir() && new_metadata.is_dir() {
        let old_names = child_names(previous)?;
        let new_names = child_names(current)?;
        let mut identical = old_names == new_names;
        for name in new_names {
            // Continue through changed directories to preserve timestamps for
            // each unchanged child, while retaining fresh times on parents.
            identical &= restore_unchanged_times(&previous.join(&name), &current.join(&name))?;
        }
        if identical {
            File::open(current)?
                .set_times(FileTimes::new().set_modified(old_metadata.modified()?))?;
        }
        return Ok(identical);
    } else if old_metadata.is_file()
        && new_metadata.is_file()
        && old_metadata.len() == new_metadata.len()
        && identical_files(previous, current)?
    {
        File::open(current)?.set_times(FileTimes::new().set_modified(old_metadata.modified()?))?;
        return Ok(true);
    } else if old_metadata.file_type().is_symlink() && new_metadata.file_type().is_symlink() {
        // Compare link text so changed destinations invalidate the containing
        // directory, without opening or adjusting the destination itself.
        return Ok(fs::read_link(previous)? == fs::read_link(current)?);
    }
    // symlink_metadata deliberately prevents traversal or modification of any
    // symlink target, whether it points inside or outside the private tree.
    Ok(false)
}

fn child_names(directory: &Path) -> io::Result<Vec<std::ffi::OsString>> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

fn identical_files(left: &Path, right: &Path) -> io::Result<bool> {
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0; 64 * 1024];
    let mut right_buffer = [0; 64 * 1024];
    loop {
        let left_count = read_chunk(&mut left, &mut left_buffer)?;
        let right_count = read_chunk(&mut right, &mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

fn read_chunk(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn io_string(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "blueprint-staging-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn work(&self) -> PathBuf {
            self.0.join("work")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_at(path: &Path, contents: &[u8], seconds: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        set_modified(path, seconds);
    }

    fn set_modified(path: &Path, seconds: u64) {
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(time(seconds)))
            .unwrap();
    }

    fn time(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn modified(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn preserves_unchanged_generated_files_and_nested_sources() {
        let fixture = Fixture::new();
        let work = fixture.work();
        let manifest = work.join("app/Cargo.toml");
        let source = work.join("app/src/main.rs");
        write_at(&manifest, b"generated final manifest", 100);
        write_at(&source, &[42; 131_073], 110);
        let guard = StagingGuard::new(&work).unwrap();
        write_at(&manifest, b"intermediate manifest", 200);
        write_at(&manifest, b"generated final manifest", 210);
        write_at(&source, &[42; 131_073], 220);
        guard.finish().unwrap();
        assert_eq!(modified(&manifest), time(100));
        assert_eq!(modified(&source), time(110));
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn preserves_new_times_for_changed_and_added_files_without_resurrecting_deleted_files() {
        let fixture = Fixture::new();
        let work = fixture.work();
        write_at(&work.join("changed"), b"old", 100);
        write_at(&work.join("deleted"), b"removed", 100);
        let guard = StagingGuard::new(&work).unwrap();
        write_at(&work.join("changed"), b"new", 200);
        write_at(&work.join("added"), b"new", 210);
        guard.finish().unwrap();
        assert_eq!(modified(&work.join("changed")), time(200));
        assert_eq!(modified(&work.join("added")), time(210));
        assert!(!work.join("deleted").exists());
    }

    #[test]
    fn preserves_directory_times_only_when_the_complete_subtree_matches() {
        let fixture = Fixture::new();
        let work = fixture.work();
        write_at(&work.join("assets/nested/texture"), b"texture", 100);
        set_modified(&work.join("assets/nested"), 110);
        set_modified(&work.join("assets"), 120);
        set_modified(&work, 130);
        let guard = StagingGuard::new(&work).unwrap();
        write_at(&work.join("assets/nested/texture"), b"texture", 200);
        set_modified(&work.join("assets/nested"), 210);
        set_modified(&work.join("assets"), 220);
        set_modified(&work, 230);
        guard.finish().unwrap();
        assert_eq!(modified(&work.join("assets/nested/texture")), time(100));
        assert_eq!(modified(&work.join("assets/nested")), time(110));
        assert_eq!(modified(&work.join("assets")), time(120));
        assert_eq!(modified(&work), time(130));
    }

    #[test]
    fn added_removed_changed_and_retyped_children_keep_new_directory_times() {
        let fixture = Fixture::new();
        let work = fixture.work();
        for directory in ["added", "removed", "changed", "retyped"] {
            write_at(&work.join(directory).join("child"), b"old", 100);
            set_modified(&work.join(directory), 110);
        }
        write_at(&work.join("removed/deleted"), b"deleted", 100);
        set_modified(&work.join("removed"), 110);
        set_modified(&work, 120);
        let guard = StagingGuard::new(&work).unwrap();
        write_at(&work.join("added/child"), b"old", 200);
        write_at(&work.join("added/new"), b"added", 200);
        write_at(&work.join("removed/child"), b"old", 200);
        write_at(&work.join("changed/child"), b"new", 200);
        fs::create_dir_all(work.join("retyped/child")).unwrap();
        for directory in ["added", "removed", "changed", "retyped"] {
            set_modified(&work.join(directory), 210);
        }
        set_modified(&work, 220);
        guard.finish().unwrap();
        for directory in ["added", "removed", "changed", "retyped"] {
            assert_eq!(modified(&work.join(directory)), time(210));
        }
        assert_eq!(modified(&work), time(220));
        assert_eq!(modified(&work.join("added/child")), time(100));
        assert!(!work.join("removed/deleted").exists());
    }

    #[test]
    fn supports_first_stage_and_cleans_snapshot_on_early_return() {
        let fixture = Fixture::new();
        let work = fixture.work();
        StagingGuard::new(&work).unwrap().finish().unwrap();
        write_at(&work.join("old"), b"old", 100);
        let guard = StagingGuard::new(&work).unwrap();
        let snapshot = guard.snapshot_dir.clone();
        assert!(snapshot.is_dir());
        drop(guard);
        assert!(!snapshot.exists());
        assert!(work.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn never_traverses_symlinks_or_restores_target_times() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let work = fixture.work();
        let outside = fixture.0.join("outside");
        write_at(&outside.join("same"), b"unchanged", 100);
        fs::create_dir_all(&work).unwrap();
        symlink(&outside, work.join("directory-link")).unwrap();
        symlink(outside.join("same"), work.join("file-link")).unwrap();
        write_at(&work.join("was-file"), b"unchanged", 100);
        let guard = StagingGuard::new(&work).unwrap();
        write_at(&outside.join("same"), b"unchanged", 200);
        symlink(&outside, work.join("directory-link")).unwrap();
        symlink(outside.join("same"), work.join("file-link")).unwrap();
        symlink(outside.join("same"), work.join("was-file")).unwrap();
        guard.finish().unwrap();
        assert_eq!(modified(&outside.join("same")), time(200));
        for name in ["directory-link", "file-link", "was-file"] {
            assert!(
                fs::symlink_metadata(work.join(name))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn changed_symlink_text_keeps_parent_time_while_identical_links_preserve_it() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let work = fixture.work();
        for directory in ["same", "changed"] {
            fs::create_dir_all(work.join(directory)).unwrap();
            symlink("old-target", work.join(directory).join("link")).unwrap();
            set_modified(&work.join(directory), 100);
        }
        let guard = StagingGuard::new(&work).unwrap();
        fs::create_dir_all(work.join("same")).unwrap();
        fs::create_dir_all(work.join("changed")).unwrap();
        symlink("old-target", work.join("same/link")).unwrap();
        symlink("new-target", work.join("changed/link")).unwrap();
        set_modified(&work.join("same"), 200);
        set_modified(&work.join("changed"), 200);
        guard.finish().unwrap();
        assert_eq!(modified(&work.join("same")), time(100));
        assert_eq!(modified(&work.join("changed")), time(200));
        assert_eq!(
            fs::read_link(work.join("changed/link")).unwrap(),
            Path::new("new-target")
        );
    }
}
