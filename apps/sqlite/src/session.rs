//! Persistent SQLx sessions. The socket owns an open transaction; losing it rolls back.
use super::*;
use axum::{body::Body, extract::Request, response::Response};
use fallible_iterator::FallibleIterator;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod timing;
use timing::RequestTiming;

const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Open {
        database: String,
        create: bool,
        read_only: bool,
    },
    Execute {
        sql: String,
        params: Vec<Value>,
        timeout_ms: u64,
    },
    Prepare {
        sql: String,
    },
    Begin {
        sql: Option<String>,
    },
    Commit,
    Rollback,
    Ping,
    Close,
}

impl Command {
    fn operation(&self) -> &'static str {
        match self {
            Self::Open { .. } => "open",
            Self::Execute { .. } => "execute",
            Self::Prepare { .. } => "prepare",
            Self::Begin { .. } => "begin",
            Self::Commit => "commit",
            Self::Rollback => "rollback",
            Self::Ping => "ping",
            Self::Close => "close",
        }
    }
}

/// Optional scalar correlation preserves legacy requests and older peers.
#[derive(Deserialize)]
struct WireCommand {
    connection_id: Option<u64>,
    request_id: Option<u64>,
    #[serde(flatten)]
    command: Command,
}

#[derive(Default, Serialize)]
struct Reply {
    results: Vec<ResultSet>,
    parameters: usize,
    declared_types: Vec<Option<String>>,
    depth: usize,
    error: Option<SqlError>,
}

#[derive(Debug, Serialize)]
struct SqlError {
    code: i32,
    message: String,
}
impl From<rusqlite::Error> for SqlError {
    fn from(e: rusqlite::Error) -> Self {
        let code = match &e {
            rusqlite::Error::SqliteFailure(e, _) => e.extended_code,
            _ => 1,
        };
        Self {
            code,
            message: e.to_string(),
        }
    }
}
impl From<String> for SqlError {
    fn from(message: String) -> Self {
        Self { code: 1, message }
    }
}

pub(super) async fn upgrade(State(service): State<Service>, mut request: Request) -> Response {
    if !request
        .headers()
        .get("upgrade")
        .is_some_and(|v| v == "trueos-sqlite-v1")
    {
        return Response::builder()
            .status(StatusCode::UPGRADE_REQUIRED)
            .body(Body::empty())
            .unwrap();
    }
    let upgraded = hyper::upgrade::on(&mut request);
    tokio::spawn(async move {
        if let Ok(socket) = upgraded.await {
            let _ = serve(service, TokioIo::new(socket)).await;
        }
    });
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("connection", "upgrade")
        .header("upgrade", "trueos-sqlite-v1")
        .body(Body::empty())
        .unwrap()
}

struct Session {
    database: Arc<Mutex<Database>>,
    transaction: Option<OwnedMutexGuard<Database>>,
    before: Vec<u8>,
    depth: usize,
    read_only: bool,
}

async fn read_command(socket: &mut TokioIo<hyper::upgrade::Upgraded>) -> Result<WireCommand, String> {
    let len = socket.read_u32().await.map_err(|e| e.to_string())? as usize;
    if len > MAX_FRAME {
        return Err("frame limit exceeded".into());
    }
    let mut bytes = vec![0; len];
    socket
        .read_exact(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn serve(
    service: Service,
    mut socket: TokioIo<hyper::upgrade::Upgraded>,
) -> Result<(), String> {
    let mut session: Option<Session> = None;
    let result = async {
        loop {
            // Bound abandoned transaction locks, while allowing idle pooled connections.
            let request = if session.as_ref().is_some_and(|s| s.depth > 0) {
                tokio::time::timeout(Duration::from_secs(60), read_command(&mut socket))
                    .await
                    .map_err(|_| "idle transaction expired".to_string())??
            } else {
                read_command(&mut socket).await?
            };
            let mut timing = RequestTiming::new(request.command.operation(), request.connection_id, request.request_id);
            let closing = matches!(request.command, Command::Close);
            let mut reply = match request.command {
                Command::Close => Ok(Reply::default()),
                Command::Open {
                    database,
                    create,
                    read_only,
                } if session.is_none() => {
                    let open = async {
                        if !valid_name(&database) {
                            return Err("invalid database name".to_string());
                        }
                        if !create
                            && storage::read(&service.directory.join(format!("{database}.sqlite")))
                                .await?
                                .is_none()
                            && !service.databases.lock().await.contains_key(&database)
                        {
                            return Err("database does not exist".into());
                        }
                        let database = service
                            .database(&database)
                            .await
                            .map_err(|(_, e)| e.0.to_string())?;
                        session = Some(Session {
                            database,
                            transaction: None,
                            before: Vec::new(),
                            depth: 0,
                            read_only,
                        });
                        Ok::<_, String>(Reply::default())
                    }
                    .await;
                    open.map_err(|message| SqlError {
                        code: if message == "database does not exist" {
                            14
                        } else {
                            1
                        },
                        message,
                    })
                }
                request => match session.as_mut() {
                    Some(s) => s.command(request, &mut timing).await,
                    None => Err("session must open a database first".to_string().into()),
                },
            }
            .unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Reply::default()
            });
            reply.depth = session.as_ref().map_or(0, |s| s.depth);
            timing.phase = "reply_serialize";
            let stage = Instant::now();
            // Emit a complete frame in one write. A short length-only write can
            // otherwise wait for delayed ACK before Nagle releases its body.
            let mut bytes = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
            let payload_len = bytes.len();
            if payload_len > MAX_FRAME {
                return Err("reply limit exceeded".into());
            }
            // The public alloc-only serializer works in every SDK feature set.
            // Reserve the prefix in this same Vec with one overlapping move.
            bytes.reserve(4);
            bytes.resize(payload_len + 4, 0);
            bytes.copy_within(..payload_len, 4);
            bytes[..4].copy_from_slice(&(payload_len as u32).to_be_bytes());
            timing.reply_serialize_us = timing::elapsed_us(stage);
            timing.reply_bytes = payload_len as u64;
            timing.phase = "reply_write";
            let stage = Instant::now();
            socket.write_all(&bytes).await.map_err(|e| e.to_string())?;
            socket.flush().await.map_err(|e| e.to_string())?;
            timing.reply_write_us = timing::elapsed_us(stage);
            timing.completed = reply.error.is_none();
            timing.phase = "complete";
            if closing {
                break;
            }
        }
        Ok(())
    }
    .await;
    if let Some(mut s) = session {
        if let Some(guard) = s.transaction.take() {
            // Rollback under the owned guard before admitting another client.
            let _ = tokio::task::spawn_blocking(move || guard.connection.execute_batch("ROLLBACK"))
                .await;
        }
    }
    result
}

impl Session {
    async fn command(&mut self, command: Command, timing: &mut RequestTiming) -> Result<Reply, SqlError> {
        if matches!(command, Command::Ping) {
            return Ok(Reply::default());
        }
        timing.phase = "lock";
        let lock_started = Instant::now();
        let guard = match self.transaction.take() {
            Some(guard) => guard,
            None => {
                let result = tokio::time::timeout(Duration::from_secs(5), self.database.clone().lock_owned()).await;
                timing.lock_us = timing::elapsed_us(lock_started);
                result.map_err(|_| SqlError {
                        code: 5,
                        message: "database is busy".into(),
                    })?
            }
        };
        timing.lock_us = timing::elapsed_us(lock_started);
        let depth = self.depth;
        let read_only = self.read_only;
        timing.phase = "run";
        let queued = Instant::now();
        let (mut guard, outcome, mut before, queue_us, before_us, execute_us) = tokio::task::spawn_blocking(move || {
            let queue_us = timing::elapsed_us(queued);
            let guard = guard;
            let mut snapshot = CommandSnapshot::default();
            let stage = Instant::now();
            let result = run(&guard.connection, command, depth, read_only, &mut snapshot);
            let execute_us = timing::elapsed_us(stage).saturating_sub(snapshot.elapsed_us);
            (guard, result, snapshot.bytes, queue_us, snapshot.elapsed_us, execute_us)
        })
        .await
        .map_err(|e| e.to_string())?;
        timing.queue_us = queue_us;
        timing.before_us = before_us;
        timing.before_bytes = before.as_ref().map_or(0, |bytes| bytes.len() as u64);
        timing.execute_us = execute_us;
        let autocommit = guard.connection.is_autocommit();
        let (reply, next_depth) = match outcome {
            Ok((reply, next_depth)) => (Ok(reply), next_depth),
            Err(e) => (Err(e), if autocommit { 0 } else { depth }),
        };
        if depth == 0 && next_depth > 0 {
            self.before = before.take().unwrap_or_default();
        }
        self.depth = next_depth;
        if next_depth > 0 {
            self.transaction = Some(guard);
            return reply;
        }
        let snapshot = if depth > 0 {
            Some(std::mem::take(&mut self.before))
        } else {
            before
        };
        // Preparing or stepping only SQLite-classified read-only statements
        // cannot change the image. Keep their cost independent of database size.
        let Some(snapshot) = snapshot else { return reply; };
        // Autocommit scripts and outermost commits publish before acknowledgement.
        if reply.is_ok() && !self.read_only {
            timing.phase = "reclaim";
            let stage = Instant::now();
            guard = tokio::task::spawn_blocking(move || {
                reclaim(&guard.connection);
                guard
            })
            .await
            .map_err(|e| e.to_string())?;
            timing.reclaim_us = timing::elapsed_us(stage);
        }
        timing.phase = "snapshot_after";
        let stage = Instant::now();
        let after = guard
            .connection
            .serialize(MAIN_DB)
            .map_err(SqlError::from)?
            .to_vec();
        timing.after_us = timing::elapsed_us(stage);
        timing.after_bytes = after.len() as u64;
        let failure = if reply.is_err() && depth == 0 {
            Some("restore failed autocommit script".to_string())
        } else if after.len() > MAX_IMAGE {
            Some("database image limit exceeded".into())
        } else if after != snapshot {
            timing.phase = "publish";
            let stage = Instant::now();
            let result = storage::publish(&guard.path, &after).await.err();
            timing.publish_us = timing::elapsed_us(stage);
            timing.publish_bytes = after.len() as u64;
            result
        } else {
            None
        };
        if let Some(message) = failure {
            timing.phase = "restore";
            let stage = Instant::now();
            let restored = tokio::task::spawn_blocking(move || {
                guard.connection = connection(&snapshot)?;
                Ok::<_, String>(guard)
            })
            .await
            .map_err(|e| e.to_string())?;
            drop(restored.map_err(SqlError::from)?);
            timing.restore_us = timing::elapsed_us(stage);
            return reply.and(Err(message.into()));
        }
        reply
    }
}

/// Snapshot before the first compiler action that may change state. Pure
/// read actions need no image; all PRAGMA/function/unknown actions retain the
/// conservative snapshot contract even when their statement is read-only.
#[derive(Default)]
struct CommandSnapshot {
    bytes: Option<Vec<u8>>,
    elapsed_us: u64,
}
impl CommandSnapshot {
    fn capture(&mut self, conn: &Connection) -> Result<(), SqlError> {
        if self.bytes.is_none() {
            let started = Instant::now();
            let result = conn.serialize(MAIN_DB).map(|bytes| bytes.to_vec()).map_err(SqlError::from);
            self.elapsed_us = self.elapsed_us.saturating_add(timing::elapsed_us(started));
            self.bytes = Some(result?);
        }
        Ok(())
    }
}

/// SQLite can apply PRAGMA effects during preparation. Ask its compiler to
/// reject every non-read action before taking the first snapshot, then retry
/// that unchanged batch cursor. This also conservatively covers SQL functions,
/// virtual-table setup and unknown future authorization actions.
struct CompileGuard {
    protected: Arc<std::sync::atomic::AtomicBool>,
    needs_snapshot: Arc<std::sync::atomic::AtomicBool>,
}
impl CompileGuard {
    fn new(conn: &Connection, execute: bool, protected: bool) -> Result<Self, SqlError> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let guard = Self { protected: Arc::new(AtomicBool::new(protected)), needs_snapshot: Arc::new(AtomicBool::new(false)) };
        let protected = guard.protected.clone();
        let needs_snapshot = guard.needs_snapshot.clone();
        conn.authorizer(Some(move |ctx: AuthContext<'_>| {
            if matches!(ctx.action, AuthAction::Attach { .. } | AuthAction::Detach { .. })
                || execute && matches!(ctx.action, AuthAction::Transaction { .. } | AuthAction::Savepoint { .. }) {
                return Authorization::Deny;
            }
            if protected.load(Ordering::Relaxed)
                && !matches!(ctx.action, AuthAction::Read { .. } | AuthAction::Select | AuthAction::Recursive) {
                needs_snapshot.store(true, Ordering::Relaxed);
                return Authorization::Deny;
            }
            Authorization::Allow
        }))?;
        Ok(guard)
    }

    fn capture(&self, conn: &Connection, snapshot: &mut CommandSnapshot) -> Result<(), SqlError> {
        // Serialization compiles an internal page-count PRAGMA. The user's
        // statement is not stepped while the temporary guard is removed.
        self.protected.store(false, std::sync::atomic::Ordering::Relaxed);
        snapshot.capture(conn)
    }

    fn next<'conn>(&self, conn: &Connection, batch: &mut rusqlite::Batch<'conn, '_>, snapshot: Option<&mut CommandSnapshot>) -> Result<Option<rusqlite::Statement<'conn>>, SqlError> {
        use std::sync::atomic::Ordering;
        let result = batch.next();
        if self.needs_snapshot.swap(false, Ordering::Relaxed) {
            drop(result);
            let snapshot = snapshot.ok_or_else(|| SqlError::from("compiler snapshot guard without an owner".to_string()))?;
            // sqlite3_serialize itself prepares an internal page-count PRAGMA.
            // Remove the temporary compiler guard before that trusted capture;
            // the rejected user statement has not run and the base policy stays.
            self.capture(conn, snapshot)?;
            // rusqlite advances Batch::tail only after successful preparation.
            batch.next().map_err(SqlError::from)
        } else {
            result.map_err(SqlError::from)
        }
    }
}

fn reset_authorizer(conn: &Connection) -> Result<(), SqlError> {
    conn.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
        _ => Authorization::Allow,
    }))?;
    Ok(())
}

fn run(
    conn: &Connection,
    command: Command,
    depth: usize,
    read_only: bool,
    snapshot: &mut CommandSnapshot,
) -> Result<(Reply, usize), SqlError> {
    match command {
        Command::Begin { sql } => {
            if depth == 0 { snapshot.capture(conn)?; }
            let statement = if depth == 0 {
                sql.unwrap_or_else(|| "BEGIN IMMEDIATE".into())
            } else {
                format!("SAVEPOINT _sqlx_savepoint_{depth}")
            };
            if depth == 0
                && !statement
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("BEGIN")
            {
                return Err("custom transaction statement must begin a transaction"
                    .to_string()
                    .into());
            }
            conn.execute_batch(&statement)?;
            if conn.is_autocommit() {
                return Err("transaction did not start".to_string().into());
            }
            Ok((Reply::default(), depth + 1))
        }
        Command::Commit | Command::Rollback => {
            if depth == 0 {
                return Ok((Reply::default(), 0));
            }
            let sql = match (matches!(command, Command::Commit), depth) {
                (true, 1) => "COMMIT".into(),
                (false, 1) => "ROLLBACK".into(),
                (true, _) => format!("RELEASE SAVEPOINT _sqlx_savepoint_{}", depth - 1),
                (false, _) => format!(
                    "ROLLBACK TO SAVEPOINT _sqlx_savepoint_{0}; RELEASE SAVEPOINT _sqlx_savepoint_{0}",
                    depth - 1
                ),
            };
            conn.execute_batch(&sql)?;
            Ok((Reply::default(), depth - 1))
        }
        Command::Prepare { sql } => {
            let guard = CompileGuard::new(conn, false, depth == 0 && snapshot.bytes.is_none())?;
            let result = (|| {
            let mut batch = rusqlite::Batch::new(conn, &sql);
            let mut reply = Reply::default();
            while let Some(stmt) = guard.next(conn, &mut batch, if depth == 0 { Some(&mut *snapshot) } else { None })? {
                reply.parameters += stmt.parameter_count();
                if reply.results.is_empty() && stmt.column_count() > 0 {
                    reply.declared_types = stmt
                        .columns()
                        .iter()
                        .map(|c| c.decl_type().map(str::to_string))
                        .collect();
                    reply.results.push(ResultSet {
                        columns: stmt.column_names().iter().map(|s| (*s).into()).collect(),
                        rows: vec![],
                        rows_affected: 0,
                        last_insert_rowid: 0,
                    });
                }
            }
            Ok((reply, depth))
            })();
            reset_authorizer(conn)?;
            result
        }
        Command::Execute {
            sql,
            params,
            timeout_ms,
        } => {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms.clamp(1, 5000));
            conn.progress_handler(1000, Some(move || Instant::now() >= deadline))?;
            // SQLx owns transaction control, so raw scripts cannot desynchronize the session.
            let guard = CompileGuard::new(conn, true, depth == 0 && snapshot.bytes.is_none())?;
            let result = execute_script(conn, &sql, params, read_only, if depth == 0 { Some(snapshot) } else { None }, &guard);
            conn.progress_handler(0, None::<fn() -> bool>)?;
            reset_authorizer(conn)?;
            result.map(|reply| (reply, depth))
        }
        _ => Err("invalid session operation".to_string().into()),
    }
}

fn execute_script(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
    read_only: bool,
    mut snapshot: Option<&mut CommandSnapshot>,
    guard: &CompileGuard,
) -> Result<Reply, SqlError> {
    let mut batch = rusqlite::Batch::new(conn, sql);
    let mut reply = Reply::default();
    let mut offset = 0;
    let mut row_count = 0;
    let mut byte_count = 0;
    while let Some(mut stmt) = guard.next(conn, &mut batch, snapshot.as_deref_mut())? {
        if read_only && !stmt.readonly() {
            return Err("read-only session".to_string().into());
        }
        if !stmt.readonly() {
            if let Some(snapshot) = snapshot.as_deref_mut() { guard.capture(conn, snapshot)?; }
        }
        let changes_before = conn.total_changes();
        let columns: Vec<String> = stmt.column_names().iter().map(|s| (*s).into()).collect();
        let mut set = ResultSet {
            columns,
            rows: vec![],
            rows_affected: 0,
            last_insert_rowid: 0,
        };
        for index in 1..=stmt.parameter_count() {
            let n = match stmt.parameter_name(index) {
                Some(name) if name.starts_with('?') || name.starts_with('$') => name[1..]
                    .parse::<usize>()
                    .map_err(|_| "unsupported parameter name".to_string())?,
                Some(_) => return Err("unsupported parameter name".to_string().into()),
                None => {
                    offset += 1;
                    offset
                }
            };
            let value = params
                .get(n.saturating_sub(1))
                .cloned()
                .unwrap_or(Value::Null)
                .sqlite();
            stmt.raw_bind_parameter(index, value)?;
        }
        {
            let mut rows = stmt.raw_query();
            while let Some(row) = rows.next()? {
                row_count += 1;
                if row_count > MAX_ROWS {
                    return Err("result row limit exceeded".to_string().into());
                }
                let mut values = Vec::new();
                for i in 0..set.columns.len() {
                    let value = match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(v) => Value::Integer(v),
                        rusqlite::types::ValueRef::Real(v) if v.is_finite() => Value::Real(v),
                        rusqlite::types::ValueRef::Real(_) => {
                            return Err("non-finite SQLite value".to_string().into());
                        }
                        rusqlite::types::ValueRef::Text(v) => {
                            Value::Text(std::str::from_utf8(v).map_err(|e| e.to_string())?.into())
                        }
                        rusqlite::types::ValueRef::Blob(v) => Value::Blob(v.to_vec()),
                    };
                    values.push(value);
                }
                byte_count += serde_json::to_vec(&values)
                    .map_err(|e| e.to_string())?
                    .len();
                if byte_count > MAX_RESULT_BYTES / 2 {
                    return Err("result byte limit exceeded".to_string().into());
                }
                set.rows.push(values);
            }
        }
        set.rows_affected = if conn.total_changes() == changes_before {
            0
        } else {
            conn.changes() as usize
        };
        set.last_insert_rowid = conn.last_insert_rowid();
        reply.results.push(set);
    }
    Ok(reply)
}

// The service owns maintenance along with the database. Reclaim at most 64 pages
// per successful potentially mutating request, preserving Codex's 16 MiB / 10% reusable reserve.
fn reclaim(conn: &Connection) {
    let stats: rusqlite::Result<(i64, i64, i64, i64)> = conn.query_row(
        "SELECT auto_vacuum, page_size, page_count, freelist_count FROM pragma_auto_vacuum, pragma_page_size, pragma_page_count, pragma_freelist_count",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
    let Ok((mode, size, pages, free)) = stats else {
        return;
    };
    if mode != 2 || size <= 0 || free * size < 64 * 1024 * 1024 || free < pages / 4 {
        return;
    }
    let reserve = (16 * 1024 * 1024 / size).max(pages / 10);
    let count = (free - reserve).clamp(0, 64);
    if count == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(100);
    let _ = conn.progress_handler(1000, Some(move || Instant::now() >= deadline));
    let _ = conn.execute_batch(&format!("PRAGMA incremental_vacuum({count})"));
    let _ = conn.progress_handler(0, None::<fn() -> bool>);
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn correlation_ids_are_only_optional_unsigned_scalars() {
        for bad in [serde_json::json!(-1), serde_json::json!("not-an-id"), serde_json::json!({"nested":1})] {
            let mut request = serde_json::json!({"op":"ping","request_id":7});
            request["connection_id"] = bad;
            assert!(serde_json::from_value::<WireCommand>(request).is_err());
        }
    }

    #[test]
    fn legacy_and_correlated_frames_decode_with_both_peer_versions() {
        for command in [
            serde_json::json!({"op":"open","database":"test","create":true,"read_only":false}),
            serde_json::json!({"op":"execute","sql":"SELECT 1","params":[],"timeout_ms":5000}),
            serde_json::json!({"op":"prepare","sql":"SELECT 1"}),
            serde_json::json!({"op":"begin","sql":null}),
            serde_json::json!({"op":"commit"}),
            serde_json::json!({"op":"rollback"}),
            serde_json::json!({"op":"ping"}),
            serde_json::json!({"op":"close"}),
        ] {
            let legacy: Command = serde_json::from_value(command.clone()).unwrap();
            let request: WireCommand = serde_json::from_value(command.clone()).unwrap();
            assert_eq!(request.command.operation(), legacy.operation());
            assert_eq!((request.connection_id, request.request_id), (None, None));
            let mut correlated = command;
            correlated["connection_id"] = 7.into();
            correlated["request_id"] = 42.into();
            let request: WireCommand = serde_json::from_value(correlated.clone()).unwrap();
            assert_eq!((request.connection_id, request.request_id), (Some(7), Some(42)));
            let older_peer: Command = serde_json::from_value(correlated).unwrap();
            assert_eq!(request.command.operation(), older_peer.operation());
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn plain_reads_prepare_without_copying_an_image() {
        let conn = connection(&[]).unwrap();
        for command in [
            Command::Prepare { sql: "SELECT 1".into() },
            Command::Execute { sql: "SELECT 1".into(), params: vec![], timeout_ms: 5000 },
        ] {
            let mut snapshot = CommandSnapshot::default();
            run(&conn, command, 0, false, &mut snapshot).unwrap();
            assert!(snapshot.bytes.is_none());
        }
    }

    #[test]
    fn pragma_and_function_actions_capture_before_preparation() {
        for sql in [
            "PRAGMA encoding='UTF-16le'", "PRAGMA schema_version=12",
            "PRAGMA application_id=17", "PRAGMA page_size=8192",
            "PRAGMA user_version=23", "PRAGMA auto_vacuum=INCREMENTAL",
            "PRAGMA journal_mode=MEMORY", "PRAGMA writable_schema=ON",
            "PRAGMA cache_size=100", "SELECT random()",
        ] {
            let conn = connection(&[]).unwrap();
            let before = conn.serialize(MAIN_DB).unwrap().to_vec();
            let mut snapshot = CommandSnapshot::default();
            run(&conn, Command::Prepare { sql: sql.into() }, 0, false, &mut snapshot).unwrap();
            assert_eq!(snapshot.bytes.as_deref(), Some(before.as_slice()), "{sql}");
        }
    }

    #[test]
    fn compiler_retry_preserves_mixed_script_order_and_initial_snapshot() {
        let conn = connection(&[]).unwrap();
        let before = conn.serialize(MAIN_DB).unwrap().to_vec();
        let mut snapshot = CommandSnapshot::default();
        let (reply, _) = run(&conn, Command::Execute {
            sql: "SELECT 1; DROP TABLE IF EXISTS absent; CREATE TABLE x(value); INSERT INTO x VALUES(7); SELECT value FROM x".into(),
            params: vec![], timeout_ms: 5000,
        }, 0, false, &mut snapshot).unwrap();
        assert_eq!(snapshot.bytes.as_deref(), Some(before.as_slice()));
        assert_eq!(reply.results.len(), 5);
        assert_eq!(conn.query_row("SELECT value FROM x", [], |r| r.get::<_, i64>(0)).unwrap(), 7);
    }
}
