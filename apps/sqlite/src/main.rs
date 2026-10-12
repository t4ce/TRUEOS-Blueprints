mod storage;
mod session;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
};
use rusqlite::{
    Connection, MAIN_DB, TransactionBehavior,
    hooks::{AuthAction, AuthContext, Authorization},
    params_from_iter,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedMutexGuard};

const MAX_IMAGE: usize = 128 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
struct Advertisement {
    protocol: &'static str,
    instance: String,
    endpoint: String,
    ipv4_endpoint: String,
    sqlite_version: String,
    storage: &'static str,
}

#[derive(Clone)]
struct Service {
    advertisement: Advertisement,
    directory: PathBuf,
    databases: Arc<Mutex<HashMap<String, Arc<Mutex<Database>>>>>,
}

struct Database {
    connection: Connection,
    path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    fn sqlite(self) -> rusqlite::types::Value {
        match self {
            Self::Null => rusqlite::types::Value::Null,
            Self::Integer(v) => rusqlite::types::Value::Integer(v),
            Self::Real(v) => rusqlite::types::Value::Real(v),
            Self::Text(v) => rusqlite::types::Value::Text(v),
            Self::Blob(v) => rusqlite::types::Value::Blob(v),
        }
    }
}

#[derive(Deserialize)]
struct Batch {
    database: String,
    statements: Vec<Statement>,
}

#[derive(Deserialize)]
struct Statement {
    sql: String,
    #[serde(default)]
    params: Vec<Value>,
}

#[derive(Serialize)]
struct ResultSet {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    rows_affected: usize,
    last_insert_rowid: i64,
}

#[derive(Serialize)]
struct BatchResult {
    results: Vec<ResultSet>,
    persisted_bytes: usize,
}

type ApiError = (StatusCode, Json<serde_json::Value>);
fn error(status: StatusCode, message: impl ToString) -> ApiError {
    (
        status,
        Json(serde_json::json!({"error": message.to_string()})),
    )
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn connection(image: &[u8]) -> Result<Connection, String> {
    if !image.is_empty() && (image.len() > MAX_IMAGE || !image.starts_with(b"SQLite format 3\0")) {
        return Err("invalid SQLite image".into());
    }
    let mut connection = Connection::open_in_memory().map_err(|e| e.to_string())?;
    if image.is_empty() {
        // Materialize an empty main database so rollback snapshots can be
        // serialized before the client's first CREATE TABLE.
        connection
            .execute_batch("PRAGMA user_version=0;")
            .map_err(|e| e.to_string())?;
    }
    if !image.is_empty() {
        connection
            .deserialize_read_exact(MAIN_DB, image, image.len(), false)
            .map_err(|e| e.to_string())?;
        let integrity: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if integrity != "ok" {
            return Err(format!("SQLite quick_check: {integrity}"));
        }
    }
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")
        .map_err(|e| e.to_string())?;
    // Databases are owned by this service; SQL cannot attach filesystem paths.
    connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

impl Service {
    async fn database(&self, name: &str) -> Result<Arc<Mutex<Database>>, ApiError> {
        if !valid_name(name) {
            return Err(error(StatusCode::BAD_REQUEST, "invalid database name"));
        }
        let mut databases = self.databases.lock().await;
        if let Some(database) = databases.get(name) {
            return Ok(database.clone());
        }
        let path = self.directory.join(format!("{name}.sqlite"));
        let bytes = storage::read(&path)
            .await
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        if bytes.as_ref().is_some_and(|b| b.is_empty()) {
            return Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "existing SQLite image is empty",
            ));
        }
        let conn =
            tokio::task::spawn_blocking(move || connection(bytes.as_deref().unwrap_or_default()))
                .await
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let database = Arc::new(Mutex::new(Database {
            connection: conn,
            path,
        }));
        databases.insert(name.into(), database.clone());
        Ok(database)
    }
}

fn execute(
    database: &mut Database,
    statements: Vec<Statement>,
) -> Result<(Vec<ResultSet>, Vec<u8>, Vec<u8>), String> {
    let before = database
        .connection
        .serialize(MAIN_DB)
        .map_err(|e| e.to_string())?
        .to_vec();
    let deadline = Instant::now() + Duration::from_secs(5);
    database
        .connection
        .progress_handler(1000, Some(move || Instant::now() >= deadline))
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let transaction = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let mut results = Vec::new();
        let mut rows_seen = 0;
        for request in statements {
            let mut statement = transaction
                .prepare(&request.sql)
                .map_err(|e| e.to_string())?;
            if statement.parameter_count() != request.params.len() {
                return Err("parameter count mismatch".into());
            }
            let params = request
                .params
                .into_iter()
                .map(Value::sqlite)
                .collect::<Vec<_>>();
            let columns = statement
                .column_names()
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>();
            let mut rows = Vec::new();
            let rows_affected = if columns.is_empty() {
                statement
                    .execute(params_from_iter(params))
                    .map_err(|e| e.to_string())?
            } else {
                let mut query = statement
                    .query(params_from_iter(params))
                    .map_err(|e| e.to_string())?;
                while let Some(row) = query.next().map_err(|e| e.to_string())? {
                    rows_seen += 1;
                    if rows_seen > MAX_ROWS {
                        return Err("result row limit exceeded".into());
                    }
                    let mut values = Vec::new();
                    for column in 0..columns.len() {
                        let value = match row.get_ref(column).map_err(|e| e.to_string())? {
                            rusqlite::types::ValueRef::Null => Value::Null,
                            rusqlite::types::ValueRef::Integer(v) => Value::Integer(v),
                            rusqlite::types::ValueRef::Real(v) if v.is_finite() => Value::Real(v),
                            rusqlite::types::ValueRef::Real(_) => {
                                return Err("non-finite SQLite value".into());
                            }
                            rusqlite::types::ValueRef::Text(v) => Value::Text(
                                std::str::from_utf8(v).map_err(|e| e.to_string())?.into(),
                            ),
                            rusqlite::types::ValueRef::Blob(v) => Value::Blob(v.to_vec()),
                        };
                        values.push(value);
                    }
                    rows.push(values);
                }
                0
            };
            results.push(ResultSet {
                columns,
                rows,
                rows_affected,
                last_insert_rowid: transaction.last_insert_rowid(),
            });
        }
        if serde_json::to_vec(&results)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_RESULT_BYTES
        {
            return Err("result byte limit exceeded".into());
        }
        transaction.commit().map_err(|e| e.to_string())?;
        let after = database
            .connection
            .serialize(MAIN_DB)
            .map_err(|e| e.to_string())?
            .to_vec();
        if after.len() > MAX_IMAGE {
            return Err("database image limit exceeded".into());
        }
        Ok((results, after))
    })();
    database
        .connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|e| e.to_string())?;
    match result {
        Ok((results, after)) => Ok((results, before, after)),
        Err(error) => {
            database.connection = connection(&before)?;
            Err(error)
        }
    }
}

async fn health(State(service): State<Service>) -> Json<Advertisement> {
    Json(service.advertisement)
}

async fn batch(
    State(service): State<Service>,
    Json(request): Json<Batch>,
) -> Result<Json<BatchResult>, ApiError> {
    // Complete admitted transactions even if the HTTP client disconnects.
    // Dropping this JoinHandle does not cancel the service's owned task.
    tokio::spawn(process_batch(service, request))
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
}

async fn process_batch(service: Service, request: Batch) -> Result<Json<BatchResult>, ApiError> {
    if request.statements.is_empty() || request.statements.len() > 128 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "batch must contain 1..128 statements",
        ));
    }
    let database = service.database(&request.database).await?;
    let guard = database.lock_owned().await;
    let (mut guard, outcome) = tokio::task::spawn_blocking(move || {
        let mut guard = guard;
        let outcome = execute(&mut guard, request.statements);
        (guard, outcome)
    })
    .await
    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (results, before, after) = outcome.map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    if let Err(persist_error) = storage::publish(&guard.path, &after).await {
        // Retain the per-database guard until memory is restored, so later
        // requests cannot observe a transaction that failed publication.
        let restored = tokio::task::spawn_blocking(move || {
            guard.connection = connection(&before)?;
            Ok::<OwnedMutexGuard<Database>, String>(guard)
        })
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        drop(restored.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?);
        return Err(error(StatusCode::INTERNAL_SERVER_ERROR, persist_error));
    }
    Ok(Json(BatchResult {
        results,
        persisted_bytes: after.len(),
    }))
}

fn port_for(instance: &str) -> u16 {
    let hash = instance.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    20_000 + (hash % 40_000) as u16
}

fn options() -> Result<(String, PathBuf), String> {
    let mut instance = "default".to_string();
    let mut directory = None;
    #[allow(unused_mut)]
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(target_os = "trueos")]
    if let Ok(script) = trueos::async_fs::block_on(trueos::async_fs::read_file(b"vFile:launch")) {
        args.extend(
            std::str::from_utf8(&script)
                .map_err(|e| e.to_string())?
                .split_whitespace()
                .map(str::to_string),
        );
    }
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--instance" | "instance" => instance = args.next().ok_or("missing instance name")?,
            "--data-dir" => {
                directory = Some(PathBuf::from(args.next().ok_or("missing data directory")?))
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if !valid_name(&instance) {
        return Err("invalid instance name".into());
    }
    let directory = directory
        .unwrap_or_else(|| PathBuf::from("/common/sqlite-service"))
        .join(&instance);
    Ok((instance, directory))
}

async fn run(instance: String, directory: PathBuf) -> Result<(), String> {
    // Binding happens first: one named instance owns one loopback port and
    // therefore one persistence directory. A competing launch fails admission.
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port_for(&instance))))
            .await
            .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    storage::create_dir(&directory).await?;
    let advertisement = Advertisement {
        protocol: "trueos-sqlite/v1",
        instance,
        endpoint: format!("http://localhost:{port}"),
        ipv4_endpoint: format!("http://127.0.0.1:{port}"),
        sqlite_version: rusqlite::version().into(),
        storage: "sqlite-memory-image",
    };
    let text = serde_json::to_vec(&advertisement).map_err(|e| e.to_string())?;
    storage::publish(&directory.join("endpoint.json"), &text).await?;
    announce(&format!(
        "sqlite-service: READY {}",
        String::from_utf8_lossy(&text)
    ));
    let service = Service {
        advertisement,
        directory,
        databases: Arc::new(Mutex::new(HashMap::new())),
    };
    let router = Router::new()
        .route("/healthz", get(health))
        .route("/v1/batch", post(batch))
        .route("/v1/connect", get(session::upgrade))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(service);
    axum::serve(listener, router)
        .await
        .map_err(|e| e.to_string())
}

fn announce(text: &str) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, text);
    #[cfg(not(target_os = "trueos"))]
    println!("{text}");
}

fn main() {
    let result = (|| {
        let (instance, directory) = options()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        tokio::task::LocalSet::new().block_on(&runtime, run(instance, directory))
    })();
    if let Err(error) = result {
        announce(&format!("sqlite-service: FAIL {error}"));
    }
}
