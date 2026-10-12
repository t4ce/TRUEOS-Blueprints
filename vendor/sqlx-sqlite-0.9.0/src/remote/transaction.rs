use super::connection::Command;
use crate::{Sqlite, SqliteConnection};
use sqlx_core::{error::Error, sql_str::SqlStr, transaction::TransactionManager};
pub struct SqliteTransactionManager;
impl TransactionManager for SqliteTransactionManager {
    type Database = Sqlite;
    async fn begin(conn: &mut SqliteConnection, statement: Option<SqlStr>) -> Result<(), Error> {
        conn.request(Command::Begin {
            sql: statement.map(|s| s.as_str().to_string()),
        })
        .await
        .map(|_| ())
    }
    async fn commit(conn: &mut SqliteConnection) -> Result<(), Error> {
        conn.request(Command::Commit).await.map(|_| ())
    }
    async fn rollback(conn: &mut SqliteConnection) -> Result<(), Error> {
        conn.request(Command::Rollback).await.map(|_| ())
    }
    fn start_rollback(conn: &mut SqliteConnection) {
        // Queue on the same session actor, before any subsequent pooled query.
        let _ = conn.enqueue(Command::Rollback);
    }
    fn get_transaction_depth(conn: &SqliteConnection) -> usize {
        conn.depth()
    }
}
