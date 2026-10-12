//! Async SQLx adapter for the localhost SQLite Blueprint. Uses the caller's Tokio runtime.
use crate::{
    Sqlite, SqliteArgumentValue, SqliteArguments, SqliteColumn, SqliteConnectOptions, SqliteError,
    SqliteQueryResult, SqliteRow, SqliteStatement, SqliteTypeInfo, SqliteValue,
};
use crate::{type_info::DataType, value::WireValue};
use futures_core::{future::BoxFuture, stream::BoxStream};
use futures_util::TryStreamExt;
pub(crate) use sqlx_core::connection::*;
use sqlx_core::{
    Either, HashMap,
    column::ColumnOrigin,
    error::Error,
    executor::{Execute, Executor},
    ext::ustr::UStr,
    sql_str::{SqlSafeStr, SqlStr},
    transaction::Transaction,
};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot},
};

pub(crate) mod collation {
    #[derive(Clone, Debug)]
    pub struct Collation;
    impl Collation {
        pub fn new<N, F>(_name: N, _collate: F) -> Self
        where
            N: Into<std::sync::Arc<str>>,
            F: Fn(&str, &str) -> std::cmp::Ordering + Send + Sync + 'static,
        {
            Self
        }
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum Command {
    Open {
        database: String,
        create: bool,
        read_only: bool,
    },
    Execute {
        sql: String,
        params: Vec<WireValue>,
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
#[derive(serde::Deserialize)]
pub(crate) struct Reply {
    pub(crate) results: Vec<ResultSet>,
    parameters: usize,
    declared_types: Vec<Option<String>>,
    depth: usize,
    error: Option<WireError>,
}
#[derive(serde::Deserialize)]
struct WireError {
    code: i32,
    message: String,
}
#[derive(serde::Deserialize)]
pub(crate) struct ResultSet {
    columns: Vec<String>,
    rows: Vec<Vec<WireValue>>,
    rows_affected: u64,
    last_insert_rowid: i64,
}

type Pending = (Command, oneshot::Sender<Result<Reply, Error>>);
pub struct SqliteConnection {
    commands: mpsc::UnboundedSender<Pending>,
    transaction_depth: Arc<AtomicUsize>,
    query_timeout: Duration,
}
// A remote service cannot expose a local C pointer or run client hook closures.
pub struct LockedSqliteHandle<'a>(std::marker::PhantomData<&'a mut SqliteConnection>);
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SqliteOperation {
    Insert,
    Update,
    Delete,
    Unknown(i32),
}
pub struct UpdateHookResult<'a> {
    pub operation: SqliteOperation,
    pub database: &'a str,
    pub table: &'a str,
    pub rowid: i64,
}

impl SqliteConnection {
    pub(crate) async fn establish(options: &SqliteConnectOptions) -> Result<Self, Error> {
        if options.vfs.is_some() || options.immutable {
            return Err(Error::Protocol(
                "remote SQLite does not support client VFS/immutable options".into(),
            ));
        }
        let endpoint = endpoint()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(protocol)?;
        let connect = || {
            client
                .get(format!("{endpoint}/v1/connect"))
                .header("connection", "upgrade")
                .header("upgrade", "trueos-sqlite-v1")
                .send()
        };
        let response = match connect().await {
            Ok(response) => response,
            Err(_first) => {
                #[cfg(target_os = "trueos")]
                {
                    ensure_service(&client, &endpoint).await?;
                    connect().await.map_err(protocol)?
                }
                #[cfg(not(target_os = "trueos"))]
                {
                    return Err(protocol(_first));
                }
            }
        };
        if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            return Err(Error::Protocol(format!(
                "SQLite service did not upgrade: {}",
                response.status()
            )));
        }
        let mut socket = response.upgrade().await.map_err(protocol)?;
        let filename = options.filename.to_string_lossy();
        let database = if options.in_memory || filename == ":memory:" {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            format!(
                "memory-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )
        } else {
            let hash = filename.bytes().fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
            format!("db-{hash:016x}")
        };
        let opened = exchange(
            &mut socket,
            &Command::Open {
                database,
                create: options.create_if_missing || options.in_memory || filename == ":memory:",
                read_only: options.read_only,
            },
        )
        .await?;
        if let Some(e) = opened.error {
            return Err(Error::Database(Box::new(SqliteError {
                code: e.code,
                message: e.message.into(),
            })));
        }
        let (commands, mut pending) = mpsc::unbounded_channel::<Pending>();
        let transaction_depth = Arc::new(AtomicUsize::new(0));
        let depth = transaction_depth.clone();
        tokio::spawn(async move {
            while let Some((command, done)) = pending.recv().await {
                let begin = matches!(command, Command::Begin { .. });
                let close = matches!(command, Command::Close);
                let result =
                    tokio::time::timeout(Duration::from_secs(30), exchange(&mut socket, &command))
                        .await
                        .unwrap_or_else(|_| {
                            Err(Error::Protocol(
                                "SQLite service response timed out; outcome may be unknown".into(),
                            ))
                        });
                if let Ok(reply) = &result {
                    depth.store(reply.depth, Ordering::Release);
                }
                let successful_begin = begin && result.is_ok();
                let result = result.and_then(|reply| {
                    if let Some(e) = &reply.error {
                        return Err(Error::Database(Box::new(SqliteError {
                            code: e.code,
                            message: e.message.clone().into(),
                        })));
                    }
                    Ok(reply)
                });
                let fatal = result
                    .as_ref()
                    .err()
                    .is_some_and(|e| !matches!(e, Error::Database(_)));
                if done.send(result).is_err() && successful_begin && !fatal {
                    // Cancellation before Transaction construction must release its server lock.
                    if let Ok(reply) = exchange(&mut socket, &Command::Rollback).await {
                        depth.store(reply.depth, Ordering::Release);
                    }
                }
                if fatal || close {
                    return;
                }
            }
            // EOF rolls back uncommitted server state, including dropped connections.
            let _ = socket.shutdown().await;
        });
        Ok(Self {
            commands,
            transaction_depth,
            query_timeout: Duration::from_secs(5),
        })
    }
    pub fn set_query_timeout(&mut self, timeout: Duration) {
        self.query_timeout = timeout;
    }
    pub fn reset_query_timeout(&mut self) {
        self.query_timeout = Duration::from_secs(5);
    }
    pub async fn lock_handle(&mut self) -> Result<LockedSqliteHandle<'_>, Error> {
        Err(Error::Protocol(
            "SQLite handle belongs to the localhost service".into(),
        ))
    }
    pub(crate) fn depth(&self) -> usize {
        self.transaction_depth.load(Ordering::Acquire)
    }
    pub(crate) fn enqueue(
        &self,
        command: Command,
    ) -> Result<oneshot::Receiver<Result<Reply, Error>>, Error> {
        let (done, result) = oneshot::channel();
        self.commands
            .send((command, done))
            .map_err(|_| Error::WorkerCrashed)?;
        Ok(result)
    }
    pub(crate) async fn request(&mut self, command: Command) -> Result<Reply, Error> {
        self.enqueue(command)?
            .await
            .map_err(|_| Error::WorkerCrashed)?
    }
    async fn execute_remote(
        &mut self,
        sql: SqlStr,
        arguments: Option<SqliteArguments>,
    ) -> Result<Vec<Either<SqliteQueryResult, SqliteRow>>, Error> {
        let params = arguments
            .map(|a| {
                a.values
                    .0
                    .into_iter()
                    .map(|v| match v {
                        SqliteArgumentValue::Null => WireValue::Null,
                        SqliteArgumentValue::Int(v) => WireValue::Integer(i64::from(v)),
                        SqliteArgumentValue::Int64(v) => WireValue::Integer(v),
                        SqliteArgumentValue::Double(v) => WireValue::Real(v),
                        SqliteArgumentValue::Text(v) => WireValue::Text((*v).clone()),
                        SqliteArgumentValue::TextSlice(v) => WireValue::Text(v.to_string()),
                        SqliteArgumentValue::Blob(v) => WireValue::Blob((*v).clone()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let reply = self
            .request(Command::Execute {
                sql: sql.as_str().to_string(),
                params,
                timeout_ms: self.query_timeout.as_millis().min(5000) as u64,
            })
            .await?;
        let mut output = Vec::new();
        for result in reply.results {
            let columns = Arc::new(columns(&result, &[]));
            let names: Arc<HashMap<UStr, usize>> = Arc::new(
                columns
                    .iter()
                    .map(|c| (c.name.clone(), c.ordinal))
                    .collect(),
            );
            for row in result.rows {
                if row.len() != columns.len() {
                    return Err(Error::Protocol("SQLite row width mismatch".into()));
                }
                let values = row
                    .into_iter()
                    .zip(columns.iter())
                    .map(|(v, c)| SqliteValue::new(v, c.type_info.clone()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                output.push(Either::Right(SqliteRow {
                    values,
                    columns: columns.clone(),
                    column_names: names.clone(),
                }));
            }
            output.push(Either::Left(SqliteQueryResult {
                changes: result.rows_affected,
                last_insert_rowid: result.last_insert_rowid,
            }));
        }
        Ok(output)
    }
}
fn protocol(e: impl fmt::Display) -> Error {
    Error::Protocol(format!("localhost SQLite: {e}"))
}
fn endpoint() -> Result<String, Error> {
    let instance = std::env::var("TRUEOS_SQLITE_INSTANCE").unwrap_or_else(|_| "codex".into());
    let hash = instance.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let endpoint = std::env::var("TRUEOS_SQLITE_ENDPOINT")
        .unwrap_or_else(|_| format!("http://localhost:{}", 20000 + hash % 40000));
    let url = url::Url::parse(&endpoint).map_err(protocol)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(Error::Protocol(
            "TRUEOS_SQLITE_ENDPOINT must be an HTTP localhost origin".into(),
        ));
    }
    // TRUEOS has an IPv4 loopback listener; avoid relying on DNS or IPv6 selection.
    Ok(format!(
        "http://127.0.0.1:{}",
        url.port_or_known_default().unwrap_or(80)
    ))
}
#[cfg(target_os = "trueos")]
async fn ensure_service(client: &reqwest::Client, endpoint: &str) -> Result<(), Error> {
    static LAUNCH: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = LAUNCH
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    if client
        .get(format!("{endpoint}/healthz"))
        .send()
        .await
        .is_ok()
    {
        return Ok(());
    }
    if std::env::var_os("TRUEOS_SQLITE_ENDPOINT").is_some() {
        return Err(Error::Protocol(
            "configured SQLite endpoint is unavailable".into(),
        ));
    }
    let instance = std::env::var("TRUEOS_SQLITE_INSTANCE").unwrap_or_else(|_| "codex".into());
    if instance.is_empty()
        || instance.len() > 64
        || !instance
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::Protocol("invalid SQLite instance name".into()));
    }
    trueos::vshell::launch_with_destination(
        "appstore",
        &format!("launch --headless sqlite -- instance {instance}\n"),
        trueos::vshell::LaunchDestination::Headless,
    )
    .map_err(protocol)?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Ok(response) = client.get(format!("{endpoint}/healthz")).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
    }
    Err(Error::Protocol(
        "SQLite Blueprint did not become ready within 30 seconds".into(),
    ))
}
async fn exchange(socket: &mut reqwest::Upgraded, command: &Command) -> Result<Reply, Error> {
    const LIMIT: usize = 8 * 1024 * 1024;
    let bytes = serde_json::to_vec(command).map_err(protocol)?;
    if bytes.len() > LIMIT {
        return Err(Error::Protocol("SQLite request limit exceeded".into()));
    }
    socket
        .write_u32(bytes.len() as u32)
        .await
        .map_err(protocol)?;
    socket.write_all(&bytes).await.map_err(protocol)?;
    socket.flush().await.map_err(protocol)?;
    let len = socket.read_u32().await.map_err(protocol)? as usize;
    if len > LIMIT {
        return Err(Error::Protocol("SQLite reply limit exceeded".into()));
    }
    let mut bytes = vec![0; len];
    socket.read_exact(&mut bytes).await.map_err(protocol)?;
    let reply: Reply = serde_json::from_slice(&bytes).map_err(protocol)?;
    Ok(reply)
}
fn columns(set: &ResultSet, declared: &[Option<String>]) -> Vec<SqliteColumn> {
    set.columns
        .iter()
        .enumerate()
        .map(|(ordinal, name)| {
            let ty = declared
                .get(ordinal)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    set.rows
                        .iter()
                        .filter_map(|r| r.get(ordinal))
                        .find_map(|v| {
                            Some(match v {
                                WireValue::Null => return None,
                                WireValue::Integer(_) => DataType::Integer,
                                WireValue::Real(_) => DataType::Float,
                                WireValue::Text(_) => DataType::Text,
                                WireValue::Blob(_) => DataType::Blob,
                            })
                        })
                        .unwrap_or(DataType::Null)
                });
            SqliteColumn {
                name: UStr::new(name),
                ordinal,
                type_info: SqliteTypeInfo(ty),
                origin: ColumnOrigin::default(),
            }
        })
        .collect()
}
impl fmt::Debug for SqliteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteSqliteConnection")
            .field("transaction_depth", &self.depth())
            .finish()
    }
}
impl Connection for SqliteConnection {
    type Database = Sqlite;
    type Options = SqliteConnectOptions;
    async fn close(mut self) -> Result<(), Error> {
        self.request(Command::Close).await.map(|_| ())
    }
    async fn close_hard(self) -> Result<(), Error> {
        drop(self);
        Ok(())
    }
    async fn ping(&mut self) -> Result<(), Error> {
        self.request(Command::Ping).await.map(|_| ())
    }
    async fn begin(&mut self) -> Result<Transaction<'_, Sqlite>, Error> {
        Transaction::begin(self, None).await
    }
    fn begin_with(
        &mut self,
        statement: impl SqlSafeStr,
    ) -> impl std::future::Future<Output = Result<Transaction<'_, Sqlite>, Error>> + Send + '_ {
        Transaction::begin(self, Some(statement.into_sql_str()))
    }
    fn cached_statements_size(&self) -> usize {
        0
    }
    async fn clear_cached_statements(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn shrink_buffers(&mut self) {}
    async fn flush(&mut self) -> Result<(), Error> {
        self.ping().await
    }
    fn should_flush(&self) -> bool {
        false
    }
}
impl<'c> Executor<'c> for &'c mut SqliteConnection {
    type Database = Sqlite;
    fn fetch_many<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxStream<'e, Result<Either<SqliteQueryResult, SqliteRow>, Error>>
    where
        'c: 'e,
        'q: 'e,
        E: Execute<'q, Sqlite> + 'q,
    {
        Box::pin(
            futures_util::stream::once(async move {
                let args = query.take_arguments().map_err(Error::Encode)?;
                self.execute_remote(query.sql(), args)
                    .await
                    .map(|rows| futures_util::stream::iter(rows.into_iter().map(Ok)))
            })
            .try_flatten(),
        )
    }
    fn fetch_optional<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxFuture<'e, Result<Option<SqliteRow>, Error>>
    where
        'c: 'e,
        'q: 'e,
        E: Execute<'q, Sqlite> + 'q,
    {
        Box::pin(async move {
            let args = query.take_arguments().map_err(Error::Encode)?;
            Ok(self
                .execute_remote(query.sql(), args)
                .await?
                .into_iter()
                .find_map(|v| match v {
                    Either::Right(row) => Some(row),
                    _ => None,
                }))
        })
    }
    fn prepare_with<'e>(
        self,
        sql: SqlStr,
        _parameters: &[SqliteTypeInfo],
    ) -> BoxFuture<'e, Result<SqliteStatement, Error>>
    where
        'c: 'e,
    {
        Box::pin(async move {
            let reply = self
                .request(Command::Prepare {
                    sql: sql.as_str().to_string(),
                })
                .await?;
            let cols = Arc::new(
                reply
                    .results
                    .first()
                    .map(|set| columns(set, &reply.declared_types))
                    .unwrap_or_default(),
            );
            let names: HashMap<UStr, usize> =
                cols.iter().map(|c| (c.name.clone(), c.ordinal)).collect();
            Ok(SqliteStatement {
                sql,
                parameters: reply.parameters,
                columns: cols,
                column_names: Arc::new(names),
            })
        })
    }
    #[cfg(feature = "offline")]
    fn describe<'e>(
        self,
        sql: SqlStr,
    ) -> BoxFuture<'e, Result<sqlx_core::describe::Describe<Sqlite>, Error>>
    where
        'c: 'e,
    {
        Box::pin(async move {
            let statement = self.prepare_with(sql, &[]).await?;
            Ok(sqlx_core::describe::Describe {
                nullable: vec![None; statement.columns.len()],
                columns: (*statement.columns).clone(),
                parameters: Some(Either::Right(statement.parameters)),
            })
        })
    }
}
