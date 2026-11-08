#[cfg(test)]
mod tests {
    use rusqlite::{Connection, OpenFlags};
    use std::path::PathBuf;
    fn initialize() {
        libsqlite3_sys::trueos_initialize().unwrap();
    }
    fn path(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("results");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
    #[test]
    fn durable_file_transactions_and_rollback() {
        initialize();
        let p = path("transactions.db");
        let _ = std::fs::remove_file(&p);
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch("CREATE TABLE entries(value TEXT); INSERT INTO entries VALUES('committed'); BEGIN; INSERT INTO entries VALUES('rolled-back'); ROLLBACK;").unwrap();
            assert_eq!(
                c.query_row("select count(*) from entries", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        let c = Connection::open_with_flags(&p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            c.query_row("select value from entries", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "committed"
        );
        assert!(
            c.execute("insert into entries values ('forbidden')", [])
                .is_err()
        );
    }
    #[test]
    fn real_lock_rejects_competing_connection_and_releases_on_close() {
        initialize();
        let p = path("locking.db");
        let _ = std::fs::remove_file(&p);
        let first = Connection::open(&p).unwrap();
        first
            .execute_batch("create table lock_test(n integer);insert into lock_test values (42)")
            .unwrap();
        assert!(Connection::open(&p).is_err());
        drop(first);
        let second = Connection::open(&p).unwrap();
        assert_eq!(
            second
                .query_row("select n from lock_test", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            42
        );
    }
    #[test]
    fn wal_commit_survives_reopen() {
        initialize();
        let p = path("wal-write.db");
        let _ = std::fs::remove_file(&p);
        {
            let c = Connection::open(&p).unwrap();
            let mode: String = c
                .query_row("pragma journal_mode=WAL", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            c.execute_batch(
                "create table wal_entries(n integer);insert into wal_entries values (7)",
            )
            .unwrap();
        }
        let c = Connection::open(&p).unwrap();
        assert_eq!(
            c.query_row("select n from wal_entries", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }
    #[test]
    fn readonly_import_recovers_committed_rows_from_existing_wal() {
        initialize();
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/fixture.db");
        let before: [Vec<u8>; 3] = [
            std::fs::read(&p).unwrap(),
            std::fs::read(p.with_file_name("fixture.db-wal")).unwrap(),
            std::fs::read(p.with_file_name("fixture.db-shm")).unwrap(),
        ];
        {
            let c = Connection::open_with_flags(&p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert_eq!(
                c.query_row("select value from records", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "native-wal-recovery-token"
            );
        }
        for (suffix, bytes) in ["", "-wal", "-shm"].iter().zip(before) {
            assert_eq!(
                std::fs::read(p.with_file_name(format!("fixture.db{suffix}"))).unwrap(),
                bytes
            );
        }
    }
    #[test]
    fn lock_is_enforced_across_processes() {
        initialize();
        let p = path("process-lock.db");
        if std::env::var_os("TRUEOS_SQLITE_LOCK_CHILD").is_some() {
            assert!(
                Connection::open(&p).is_err(),
                "another process must observe the native File lock"
            );
            return;
        }
        let _ = std::fs::remove_file(&p);
        let owner = Connection::open(&p).unwrap();
        owner
            .execute_batch("create table cross_process(n integer)")
            .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::lock_is_enforced_across_processes",
                "--nocapture",
            ])
            .env("TRUEOS_SQLITE_LOCK_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
        drop(owner);
        Connection::open(&p).unwrap();
    }
    #[test]
    fn sqlite_temp_sort_and_large_blob_roundtrip() {
        initialize();
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("create table blobs(b blob); insert into blobs values (zeroblob(262144));")
            .unwrap();
        assert_eq!(
            c.query_row("select length(b) from blobs order by b", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            262144
        );
    }
}
