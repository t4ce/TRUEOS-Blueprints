use sqlx::{
    ConnectOptions, Connection, Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::time::Duration;
include!(concat!(env!("OUT_DIR"), "/migrators.rs"));

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = SqliteConnectOptions::new()
        .filename("/common/codex/sqlx-probe.sqlite")
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options.clone())
        .await?;
    sqlx::raw_sql("DROP TABLE IF EXISTS probe; CREATE TABLE probe (id INTEGER PRIMARY KEY, number INTEGER, real REAL, text TEXT, blob BLOB, absent TEXT);").execute(&pool).await?;
    let done = sqlx::query("INSERT INTO probe VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(1i64)
        .bind(i64::MAX)
        .bind(1.25f64)
        .bind("hello 🌍")
        .bind(vec![0u8, 255, 7])
        .bind(Option::<String>::None)
        .execute(&pool)
        .await?;
    assert_eq!(done.rows_affected(), 1);
    assert_eq!(done.last_insert_rowid(), 1);
    let row = sqlx::query("SELECT * FROM probe").fetch_one(&pool).await?;
    assert_eq!(row.try_get::<i64, _>("number")?, i64::MAX);
    assert_eq!(row.try_get::<f64, _>("real")?, 1.25);
    assert_eq!(row.try_get::<String, _>("text")?, "hello 🌍");
    assert_eq!(row.try_get::<Vec<u8>, _>("blob")?, [0, 255, 7]);
    assert_eq!(row.try_get::<Option<String>, _>("absent")?, None);
    {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE probe SET number = 10")
            .execute(&mut *tx)
            .await?;
        {
            let mut nested = tx.begin().await?;
            sqlx::query("UPDATE probe SET number = 20")
                .execute(&mut *nested)
                .await?;
            nested.rollback().await?;
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT number FROM probe")
                .fetch_one(&mut *tx)
                .await?,
            10
        );
        tx.commit().await?;
    }
    {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE probe SET number = 30")
            .execute(&mut *tx)
            .await?;
        drop(tx);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT number FROM probe")
            .fetch_one(&pool)
            .await?,
        10
    );
    // Closing a socket with an open transaction rolls back in the service.
    let mut connection = options.connect().await?;
    let mut tx = connection.begin().await?;
    sqlx::query("UPDATE probe SET number = 40")
        .execute(&mut *tx)
        .await?;
    std::mem::forget(tx);
    connection.close_hard().await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT number FROM probe")
            .fetch_one(&pool)
            .await?,
        10
    );
    let error = sqlx::query("INSERT INTO probe(id) VALUES (1)")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(error.as_database_error().unwrap().is_unique_violation());
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            sqlx::query("UPDATE probe SET number = number + 1")
                .execute(&pool)
                .await
        }));
    }
    for task in tasks {
        task.await??;
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT number FROM probe")
            .fetch_one(&pool)
            .await?,
        26
    );
    let mut connection = pool.acquire().await?;
    connection.set_query_timeout(Duration::from_millis(1));
    let error = sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n")
        .fetch_one(&mut *connection).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("9")
    );
    connection.reset_query_timeout();
    drop(connection);
    for (directory, migrator) in migrators() {
        let options = SqliteConnectOptions::new()
            .filename(format!("/common/codex/{directory}.sqlite"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        migrator.run(&pool).await?;
        migrator.run(&pool).await?;
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await?,
            "ok"
        );
        println!("sqlx-client: migrations={directory} PASS");
        pool.close().await;
    }
    assert!(<sqlx::Sqlite as sqlx::migrate::MigrateDatabase>::database_exists("sqlite:///common/codex/sqlx-probe.sqlite").await?);
    assert!(!<sqlx::Sqlite as sqlx::migrate::MigrateDatabase>::database_exists("sqlite:///common/codex/not-created.sqlite").await?);
    let mut read_only = options.clone().read_only(true).create_if_missing(false).connect().await?;
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT number FROM probe").fetch_one(&mut read_only).await?, 26);
    assert!(sqlx::query("UPDATE probe SET number = 0").execute(&mut read_only).await.is_err());
    read_only.close().await?;
    pool.close().await;
    let mut connection = options.connect().await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT number FROM probe")
            .fetch_one(&mut connection)
            .await?,
        26
    );
    connection.close().await?;
    #[cfg(target_os = "trueos")]
    trueos::async_fs::write_file(b"/common/sqlx-client-report.json", br#"{"result":"PASS","typed_values":true,"nested_transactions":true,"disconnect_rollback":true,"concurrent_updates":16,"codex_migration_sets":6}"#).await.map_err(|e| format!("report persistence: {e}"))?;
    println!("sqlx-client: PASS");
    Ok(())
}
fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    if let Err(e) = tokio::task::LocalSet::new().block_on(&rt, run()) {
        #[cfg(target_os = "trueos")]
        {
            let report = serde_json::to_vec(&serde_json::json!({"result":"FAIL", "error":e.to_string()})).unwrap();
            let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(b"/common/sqlx-client-report.json", &report));
            trueos::logl::log(trueos::logl::level::ERROR, &format!("sqlx-client: FAIL {e}"));
        }
        eprintln!("sqlx-client: FAIL {e}");
        std::process::exit(1);
    }
}
