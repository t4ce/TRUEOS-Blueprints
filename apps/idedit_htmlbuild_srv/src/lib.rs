//! IDEdit HTML development preview. Sources are complete redb revisions in RAM.
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use std::sync::{Arc, RwLock};
use trueos_ide::{
    LoadedSource,
    html_preview::{REVISION_MARKER, valid_asset_path},
};

pub const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct Preview {
    entry: String,
    source: Arc<RwLock<Arc<LoadedSource>>>,
}
impl Preview {
    pub fn new(entry: String, source: LoadedSource) -> Result<Self, String> {
        validate(&entry, &source)?;
        Ok(Self {
            entry,
            source: Arc::new(RwLock::new(Arc::new(source))),
        })
    }
    pub fn replace(&self, source: LoadedSource) -> Result<(), String> {
        validate(&self.entry, &source)?;
        *self.source.write().unwrap() = Arc::new(source);
        Ok(())
    }
    pub fn revision(&self) -> u64 {
        self.source.read().unwrap().revision.revision
    }
}
fn validate(entry: &str, source: &LoadedSource) -> Result<(), String> {
    if !valid_asset_path(entry)
        || source.file(entry).is_none()
        || source
            .files
            .iter()
            .any(|file| !valid_asset_path(&file.path) || file.bytes.len() > MAX_ASSET_BYTES)
        || source
            .files
            .iter()
            .map(|file| file.bytes.len())
            .sum::<usize>()
            > MAX_SOURCE_BYTES
    {
        return Err("Invalid, missing or oversized HTML project sources".into());
    }
    Ok(())
}

pub fn router(preview: Preview) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/__idedit/revision", get(revision))
        .route("/{*path}", get(asset))
        .with_state(preview)
}
async fn revision(State(preview): State<Preview>) -> Response {
    reply(
        StatusCode::OK,
        "text/plain",
        preview.revision().to_string().into_bytes(),
    )
}
async fn index(State(preview): State<Preview>) -> Response {
    serve(&preview, &preview.entry)
}
async fn asset(State(preview): State<Preview>, Path(path): Path<String>) -> Response {
    serve(&preview, &path)
}
fn serve(preview: &Preview, path: &str) -> Response {
    if !valid_asset_path(path) {
        return reply(
            StatusCode::BAD_REQUEST,
            "text/plain",
            b"Invalid asset path".to_vec(),
        );
    }
    // Clone one immutable source handle; a concurrent Save cannot tear a response.
    let source = preview.source.read().unwrap().clone();
    let Some(file) = source.file(path) else {
        return reply(StatusCode::NOT_FOUND, "text/plain", b"Not found".to_vec());
    };
    let mime = content_type(path.rsplit('.').next().unwrap_or(""));
    let mut bytes = Vec::new();
    if mime.starts_with("text/html") {
        bytes.extend_from_slice(
            format!("{REVISION_MARKER}{} -->\n", source.revision.revision).as_bytes(),
        );
    }
    bytes.extend_from_slice(&file.bytes);
    reply(StatusCode::OK, mime, bytes)
}
fn reply(status: StatusCode, mime: &str, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .unwrap()
}

fn content_type(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
