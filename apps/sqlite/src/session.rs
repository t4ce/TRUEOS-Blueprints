//! Persistent SQLx sessions. The socket owns an open transaction; losing it rolls back.
use super::*;
use axum::{body::Body, extract::Request, response::Response};
use fallible_iterator::FallibleIterator;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

async fn read_command(socket: &mut TokioIo<hyper::upgrade::Upgraded>) -> Result<Command, String> {
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
            let closing = matches!(request, Command::Close);
            let mut reply = match request {
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
                    Some(s) => s.command(request).await,
                    None => Err("session must open a database first".to_string().into()),
                },
            }
            .unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Reply::default()
            });
            reply.depth = session.as_ref().map_or(0, |s| s.depth);
            let bytes = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
            if bytes.len() > MAX_FRAME {
                return Err("reply limit exceeded".into());
            }
            socket
                .write_u32(bytes.len() as u32)
                .await
                .map_err(|e| e.to_string())?;
            socket.write_all(&bytes).await.map_err(|e| e.to_string())?;
            socket.flush().await.map_err(|e| e.to_string())?;
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
    async fn command(&mut self, command: Command) -> Result<Reply, SqlError> {
        if matches!(command, Command::Ping) {
            return Ok(Reply::default());
        }
        let guard = match self.transaction.take() {
            Some(guard) => guard,
            None => {
                tokio::time::timeout(Duration::from_secs(5), self.database.clone().lock_owned())
                    .await
                    .map_err(|_| SqlError {
                        code: 5,
                        message: "database is busy".into(),
                    })?
            }
        };
        let depth = self.depth;
        let read_only = self.read_only;
        let (mut guard, outcome, before) = tokio::task::spawn_blocking(move || {
            let guard = guard;
            let before = if depth == 0 {
                guard
                    .connection
                    .serialize(MAIN_DB)
                    .map(|b| b.to_vec())
                    .map_err(SqlError::from)
            } else {
                Ok(Vec::new())
            };
            let result = before
                .as_ref()
                .map_err(|e| SqlError {
                    code: e.code,
                    message: e.message.clone(),
                })
                .and_then(|_| run(&guard.connection, command, depth, read_only));
            (guard, result, before.unwrap_or_default())
        })
        .await
        .map_err(|e| e.to_string())?;
        let autocommit = guard.connection.is_autocommit();
        let (reply, next_depth) = match outcome {
            Ok((reply, next_depth)) => (Ok(reply), next_depth),
            Err(e) => (Err(e), if autocommit { 0 } else { depth }),
        };
        if depth == 0 && next_depth > 0 {
            self.before = before.clone();
        }
        self.depth = next_depth;
        if next_depth > 0 {
            self.transaction = Some(guard);
            return reply;
        }
        let snapshot = if depth > 0 {
            std::mem::take(&mut self.before)
        } else {
            before
        };
        // Autocommit scripts and outermost commits publish before acknowledgement.
        if reply.is_ok() && !self.read_only {
            guard = tokio::task::spawn_blocking(move || {
                reclaim(&guard.connection);
                guard
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        let after = guard
            .connection
            .serialize(MAIN_DB)
            .map_err(SqlError::from)?
            .to_vec();
        let failure = if reply.is_err() && depth == 0 {
            Some("restore failed autocommit script".to_string())
        } else if after.len() > MAX_IMAGE {
            Some("database image limit exceeded".into())
        } else if after != snapshot {
            storage::publish(&guard.path, &after).await.err()
        } else {
            None
        };
        if let Some(message) = failure {
            let restored = tokio::task::spawn_blocking(move || {
                guard.connection = connection(&snapshot)?;
                Ok::<_, String>(guard)
            })
            .await
            .map_err(|e| e.to_string())?;
            drop(restored.map_err(SqlError::from)?);
            return reply.and(Err(message.into()));
        }
        reply
    }
}

fn run(
    conn: &Connection,
    command: Command,
    depth: usize,
    read_only: bool,
) -> Result<(Reply, usize), SqlError> {
    match command {
        Command::Begin { sql } => {
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
            let mut batch = rusqlite::Batch::new(conn, &sql);
            let mut reply = Reply::default();
            while let Some(stmt) = batch.next()? {
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
        }
        Command::Execute {
            sql,
            params,
            timeout_ms,
        } => {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms.clamp(1, 5000));
            conn.progress_handler(1000, Some(move || Instant::now() >= deadline))?;
            // SQLx owns transaction control, so raw scripts cannot desynchronize the session.
            conn.authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Attach { .. }
                | AuthAction::Detach { .. }
                | AuthAction::Transaction { .. }
                | AuthAction::Savepoint { .. } => Authorization::Deny,
                _ => Authorization::Allow,
            }))?;
            let result = execute_script(conn, &sql, params, read_only);
            conn.progress_handler(0, None::<fn() -> bool>)?;
            conn.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
                _ => Authorization::Allow,
            }))?;
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
) -> Result<Reply, SqlError> {
    let mut batch = rusqlite::Batch::new(conn, sql);
    let mut reply = Reply::default();
    let mut offset = 0;
    let mut row_count = 0;
    let mut byte_count = 0;
    while let Some(mut stmt) = batch.next()? {
        if read_only && !stmt.readonly() {
            return Err("read-only session".to_string().into());
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
// per successful request, preserving Codex's 16 MiB / 10% reusable reserve.
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
