mod html;
mod tree;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod system_services;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod webdevices;

use std::env;
use std::net::SocketAddr;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    Form, Json, Router,
    extract::{Multipart, Path as AxumPath, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use {
    hyper::service::service_fn,
    tokio::io::{AsyncRead, AsyncWrite, ReadBuf},
    tower::Service,
};

use tree::{resolve_under_root, scan_dir};
use websys::{
    JobQueue,
    path::{Path, PathBuf},
};

const PORT: u16 = 54321;

#[derive(Clone)]
struct AppState {
    root: PathBuf,
    common_root: Option<PathBuf>,
    app_jobs: JobQueue,
    common_jobs: Option<JobQueue>,
}

#[derive(Clone, Copy)]
enum RootScope {
    App,
    Common,
}

struct RootContext<'a> {
    scope: RootScope,
    root: &'a Path,
    jobs: &'a JobQueue,
}

impl RootScope {
    fn tree_base(self) -> &'static str {
        match self {
            Self::App => "/tree",
            Self::Common => "/common/tree",
        }
    }

    fn file_base(self) -> &'static str {
        match self {
            Self::App => "",
            Self::Common => "/common/files",
        }
    }

    fn jobs_base(self) -> &'static str {
        match self {
            Self::App => "/jobs",
            Self::Common => "/common/jobs",
        }
    }

    fn sha_base(self) -> &'static str {
        match self {
            Self::App => "/api/sha256",
            Self::Common => "/common/api/sha256",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::App => "app://",
            Self::Common => "common://",
        }
    }
}

#[derive(Deserialize)]
struct MoveJobForm {
    source: String,
    destination: String,
}

#[derive(Deserialize)]
struct DeleteJobForm {
    target: String,
}

#[derive(Deserialize)]
struct DownloadJobForm {
    source: String,
}

#[derive(Deserialize)]
struct ArchiveJobForm {
    sources: String,
    directory: String,
    name: String,
}

#[derive(Deserialize)]
struct SelectionJobForm {
    sources: String,
}

#[derive(Deserialize)]
struct ExtractJobForm {
    archive: String,
    destination: String,
}

#[derive(Serialize)]
struct ShaResponse {
    algorithm: &'static str,
    path: String,
    sha256: String,
    bytes: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let port = env::var("TRUEOS_APP_FS_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(PORT);
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_root);

    let root = normalize_root(root);
    let common_root = default_common_root().map(normalize_root);

    if let Err(err) = tokio::fs::create_dir_all(&root).await {
        eprintln!("app root setup failed under {}: {err}", root.display());
    }

    if let Some(common_root) = common_root.as_ref()
        && let Err(err) = tokio::fs::create_dir_all(common_root).await
    {
        eprintln!(
            "common root setup failed under {}: {err}",
            common_root.display()
        );
    }

    let state = AppState {
        root: root.clone(),
        common_root: common_root.clone(),
        app_jobs: JobQueue::new(root.clone()),
        common_jobs: common_root.clone().map(JobQueue::new),
    };
    let app = Router::new()
        .route("/", get(root_overview))
        .route("/tree", get(tree_root))
        .route("/tree/{*path}", get(tree_subdir))
        .route("/common", get(common_tree_root))
        .route("/common/tree", get(common_tree_root))
        .route("/common/tree/{*path}", get(common_tree_subdir))
        .route("/healthz", get(healthz))
        .route("/api/healthz", get(healthz))
        .route("/ui/style.css", get(stylesheet))
        .route("/api/sha256/{*path}", get(file_sha256))
        .route("/common/api/sha256/{*path}", get(common_file_sha256))
        .route("/common/files/{*path}", get(serve_common_file))
        .route("/jobs", get(jobs_index))
        .route("/jobs/move", post(submit_move))
        .route("/jobs/delete", post(submit_delete))
        .route("/jobs/upload", post(submit_upload))
        .route("/jobs/download", post(submit_download))
        .route("/jobs/archive", post(submit_archive))
        .route("/jobs/download-selection", post(submit_selection_download))
        .route("/jobs/extract", post(submit_extract))
        .route("/jobs/{id}", get(job_detail))
        .route("/common/jobs", get(common_jobs_index))
        .route("/common/jobs/move", post(submit_common_move))
        .route("/common/jobs/delete", post(submit_common_delete))
        .route("/common/jobs/upload", post(submit_common_upload))
        .route("/common/jobs/download", post(submit_common_download))
        .route("/common/jobs/archive", post(submit_common_archive))
        .route(
            "/common/jobs/download-selection",
            post(submit_common_selection_download),
        )
        .route("/common/jobs/extract", post(submit_common_extract))
        .route("/common/jobs/{id}", get(common_job_detail))
        .route("/{*path}", get(serve_app_file))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|error| {
            eprintln!("failed to bind {addr}: {error}");
            std::process::exit(1);
        });

    println!("Serving {} at http://{addr}/", root.display());
    if let Some(common_root) = common_root.as_ref() {
        println!(
            "Common root {} ({}) at http://{addr}/common/tree",
            RootScope::Common.label(),
            common_root.display()
        );
    }
    println!("File tree: http://{addr}/tree");
    println!("Job queue: http://{addr}/jobs");

    serve_all(listener, app).await.unwrap_or_else(|error| {
        eprintln!("server error: {error}");
        std::process::exit(1);
    });
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn serve_all(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(async {
                if let Err(error) = webdevices::serve().await {
                    trueos::logl::log(
                        trueos::logl::level::ERROR,
                        format_args!("webdevices-http: runtime failed {error:?}"),
                    );
                }
            });
            tokio::task::spawn_local(async {
                if let Err(error) = system_services::serve().await {
                    trueos::logl::log(
                        trueos::logl::level::ERROR,
                        format_args!("system-services: runtime failed {error:?}"),
                    );
                }
            });
            serve_http(listener, app).await
        })
        .await
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
async fn serve_all(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    serve_http(listener, app).await
}

/// Axum's generic server uses hyper-util protocol auto-detection, which reads
/// the first 24 bytes before handing an HTTP/1 stream to Hyper. On the TrueOS
/// Blueprint socket bridge that handoff can park after `EAGAIN`. FileSystem is
/// HTTP/1-only, so use Hyper's direct HTTP/1 driver, matching PlotTwist's proven
/// Blueprint server pattern while retaining the Axum Router and handlers.
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn serve_http(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let app = app.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request| {
                let mut app = app.clone();
                async move { app.call(request.map(axum::body::Body::new)).await }
            });

            if let Err(error) = hyper::server::conn::http1::Builder::new()
                .serve_connection(HyperIo(stream), service)
                .await
            {
                eprintln!("HTTP/1 connection from {peer} failed: {error:?}");
            }
        });
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
async fn serve_http(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(listener, app).await
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
struct HyperIo<T>(T);

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn hyper_io_error(_: std::io::Error) -> hyper::io::Error {
    hyper::io::Error::new(hyper::io::ErrorKind::Other, "tokio io error")
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: AsyncRead + Unpin> hyper::rt::Read for HyperIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        mut output: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), hyper::io::Error>> {
        let read = unsafe {
            let mut input = ReadBuf::uninit(output.as_mut());
            match Pin::new(&mut self.0).poll_read(context, &mut input) {
                Poll::Ready(Ok(())) => input.filled().len(),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(hyper_io_error(error))),
                Poll::Pending => return Poll::Pending,
            }
        };
        unsafe { output.advance(read) };
        Poll::Ready(Ok(()))
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, hyper::io::Error>> {
        Pin::new(&mut self.0)
            .poll_write(context, bytes)
            .map_err(hyper_io_error)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), hyper::io::Error>> {
        Pin::new(&mut self.0)
            .poll_flush(context)
            .map_err(hyper_io_error)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), hyper::io::Error>> {
        Pin::new(&mut self.0)
            .poll_shutdown(context)
            .map_err(hyper_io_error)
    }
}

fn normalize_root(root: PathBuf) -> PathBuf {
    let text = root.display().to_string();
    if text.trim().is_empty() {
        PathBuf::from(".")
    } else {
        root
    }
}

fn default_root() -> PathBuf {
    env::var("TRUEOS_APP_FS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn default_common_root() -> Option<PathBuf> {
    env::var("TRUEOS_APP_COMMON")
        .or_else(|_| env::var("TRUEOS_APP_FS_COMMON"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

async fn tree_root(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let context = app_context(&state);
    render_tree(context, "").await
}

async fn root_overview(State(state): State<AppState>) -> Html<String> {
    Html(html::render_root_overview_page(
        &state.root,
        state.common_root.as_deref(),
    ))
}

async fn tree_subdir(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Html<String>, AppError> {
    let dir = resolve_under_root(&state.root, &path).ok_or(AppError::NotFound)?;
    let context = RootContext {
        scope: RootScope::App,
        root: &dir,
        jobs: &state.app_jobs,
    };
    render_tree(context, &path).await
}

async fn common_tree_root(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let context = common_context(&state, None)?;
    render_tree(context, "").await
}

async fn common_tree_subdir(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Html<String>, AppError> {
    let root = state.common_root.as_ref().ok_or(AppError::NotFound)?;
    let dir = resolve_under_root(root, &path).ok_or(AppError::NotFound)?;
    let context = common_context(&state, Some(&dir))?;
    render_tree(context, &path).await
}

fn app_context(state: &AppState) -> RootContext<'_> {
    RootContext {
        scope: RootScope::App,
        root: &state.root,
        jobs: &state.app_jobs,
    }
}

fn common_context<'a>(
    state: &'a AppState,
    root_override: Option<&'a Path>,
) -> Result<RootContext<'a>, AppError> {
    let root = root_override
        .or(state.common_root.as_deref())
        .ok_or(AppError::NotFound)?;
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    Ok(RootContext {
        scope: RootScope::Common,
        root,
        jobs,
    })
}

async fn render_tree(context: RootContext<'_>, rel: &str) -> Result<Html<String>, AppError> {
    let mut nodes = scan_dir(context.root, rel).await?;
    prefix_file_urls(&mut nodes, context.scope.file_base());
    let jobs = context.jobs.list(8).await;
    Ok(Html(html::render_tree_page(
        context.root,
        rel,
        &nodes,
        &jobs,
        html::TreeLinks {
            scope_label: context.scope.label(),
            tree_base: context.scope.tree_base(),
            file_base: context.scope.file_base(),
            jobs_base: context.scope.jobs_base(),
            sha_base: context.scope.sha_base(),
        },
    )))
}

fn prefix_file_urls(nodes: &mut [tree::TreeNode], prefix: &str) {
    for node in nodes {
        match node {
            tree::TreeNode::Dir { children, .. } => prefix_file_urls(children, prefix),
            tree::TreeNode::File { url_path, .. } => {
                let trimmed = url_path.trim_start_matches('/');
                *url_path = if prefix.is_empty() {
                    format!("/{trimmed}")
                } else {
                    format!("{}/{}", prefix.trim_end_matches('/'), trimmed)
                };
            }
        }
    }
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        html::stylesheet(),
    )
}

async fn healthz() -> &'static str {
    "ok"
}

async fn serve_app_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    serve_file_under(&state.root, &path).await
}

async fn serve_common_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    let root = state.common_root.as_ref().ok_or(AppError::NotFound)?;
    serve_file_under(root, &path).await
}

async fn serve_file_under(root: &Path, request_path: &str) -> Result<Response, AppError> {
    let mut file = resolve_under_root(root, request_path).ok_or(AppError::NotFound)?;
    let metadata = tokio::fs::metadata(&file)
        .await
        .map_err(|_| AppError::NotFound)?;
    if metadata.is_dir() {
        file = file.join("index.html");
    } else if !metadata.is_file() {
        return Err(AppError::NotFound);
    }

    let bytes = tokio::fs::read(&file)
        .await
        .map_err(|_| AppError::NotFound)?;
    let content_type = content_type_for(&file);
    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

fn content_type_for(path: &Path) -> &'static str {
    let name = path.to_str().unwrap_or_default();
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("txt" | "md" | "rs" | "toml" | "yaml" | "yml") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("7z") => "application/x-7z-compressed",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

async fn jobs_index(State(state): State<AppState>) -> Html<String> {
    render_jobs_index(app_context(&state)).await
}

async fn job_detail(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<u64>,
) -> Result<Html<String>, AppError> {
    render_job_detail(app_context(&state), id).await
}

async fn common_jobs_index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    Ok(render_jobs_index(common_context(&state, None)?).await)
}

async fn common_job_detail(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<u64>,
) -> Result<Html<String>, AppError> {
    render_job_detail(common_context(&state, None)?, id).await
}

async fn render_jobs_index(context: RootContext<'_>) -> Html<String> {
    let jobs = context.jobs.list(64).await;
    Html(html::render_jobs_page(
        context.root,
        &jobs,
        html::TreeLinks {
            scope_label: context.scope.label(),
            tree_base: context.scope.tree_base(),
            file_base: context.scope.file_base(),
            jobs_base: context.scope.jobs_base(),
            sha_base: context.scope.sha_base(),
        },
    ))
}

async fn render_job_detail(context: RootContext<'_>, id: u64) -> Result<Html<String>, AppError> {
    let job = context.jobs.get(id).await.ok_or(AppError::NotFound)?;
    Ok(Html(html::render_job_page(
        context.root,
        &job,
        html::TreeLinks {
            scope_label: context.scope.label(),
            tree_base: context.scope.tree_base(),
            file_base: context.scope.file_base(),
            jobs_base: context.scope.jobs_base(),
            sha_base: context.scope.sha_base(),
        },
    )))
}

async fn submit_move(
    State(state): State<AppState>,
    Form(form): Form<MoveJobForm>,
) -> Result<Redirect, AppError> {
    submit_move_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_move(
    State(state): State<AppState>,
    Form(form): Form<MoveJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_move_to(jobs, RootScope::Common, form).await
}

async fn submit_move_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: MoveJobForm,
) -> Result<Redirect, AppError> {
    let job = jobs
        .enqueue_move(form.source, form.destination)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

async fn submit_delete(
    State(state): State<AppState>,
    Form(form): Form<DeleteJobForm>,
) -> Result<Redirect, AppError> {
    submit_delete_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_delete(
    State(state): State<AppState>,
    Form(form): Form<DeleteJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_delete_to(jobs, RootScope::Common, form).await
}

async fn submit_delete_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: DeleteJobForm,
) -> Result<Redirect, AppError> {
    let job = jobs
        .enqueue_delete(form.target)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

async fn submit_download(
    State(state): State<AppState>,
    Form(form): Form<DownloadJobForm>,
) -> Result<Redirect, AppError> {
    submit_download_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_download(
    State(state): State<AppState>,
    Form(form): Form<DownloadJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_download_to(jobs, RootScope::Common, form).await
}

async fn submit_download_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: DownloadJobForm,
) -> Result<Redirect, AppError> {
    let job = jobs
        .enqueue_download(form.source)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

async fn submit_archive(
    State(state): State<AppState>,
    Form(form): Form<ArchiveJobForm>,
) -> Result<Redirect, AppError> {
    submit_archive_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_archive(
    State(state): State<AppState>,
    Form(form): Form<ArchiveJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_archive_to(jobs, RootScope::Common, form).await
}

async fn submit_archive_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: ArchiveJobForm,
) -> Result<Redirect, AppError> {
    let sources = selected_sources(&form.sources)?;
    let destination = archive_destination(&form.directory, &form.name)?;
    let job = jobs
        .enqueue_archive(sources, destination, false)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

async fn submit_selection_download(
    State(state): State<AppState>,
    Form(form): Form<SelectionJobForm>,
) -> Result<Redirect, AppError> {
    submit_selection_download_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_selection_download(
    State(state): State<AppState>,
    Form(form): Form<SelectionJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_selection_download_to(jobs, RootScope::Common, form).await
}

async fn submit_selection_download_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: SelectionJobForm,
) -> Result<Redirect, AppError> {
    let sources = selected_sources(&form.sources)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination = format!(
        "{}/selection-{stamp}.7z",
        websys::jobs::DOWNLOAD_STAGING_DIR
    );
    let job = jobs
        .enqueue_archive(sources, destination, true)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

async fn submit_extract(
    State(state): State<AppState>,
    Form(form): Form<ExtractJobForm>,
) -> Result<Redirect, AppError> {
    submit_extract_to(&state.app_jobs, RootScope::App, form).await
}

async fn submit_common_extract(
    State(state): State<AppState>,
    Form(form): Form<ExtractJobForm>,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_extract_to(jobs, RootScope::Common, form).await
}

async fn submit_extract_to(
    jobs: &JobQueue,
    scope: RootScope,
    form: ExtractJobForm,
) -> Result<Redirect, AppError> {
    let job = jobs
        .enqueue_extract(form.archive, form.destination)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

fn selected_sources(json: &str) -> Result<Vec<String>, AppError> {
    serde_json::from_str(json)
        .map_err(|error| AppError::BadRequest(format!("invalid file selection: {error}")))
}

fn archive_destination(directory: &str, name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(AppError::BadRequest(
            "archive name must be one file name".to_string(),
        ));
    }
    let name = if name.ends_with(".7z") {
        name.to_string()
    } else {
        format!("{name}.7z")
    };
    let directory = directory.trim().trim_matches('/');
    if directory.is_empty() || directory == "." {
        return Err(AppError::BadRequest(
            "archive store path cannot be empty; choose a directory inside this root".to_string(),
        ));
    }
    if directory.contains('\\')
        || directory
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AppError::BadRequest(
            "archive store path must be a forward-slash relative directory inside this root"
                .to_string(),
        ));
    }
    Ok(format!("{directory}/{name}"))
}

async fn file_sha256(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Json<ShaResponse>, AppError> {
    sha256_under(&state.root, path).await
}

async fn common_file_sha256(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Result<Json<ShaResponse>, AppError> {
    let root = state.common_root.as_ref().ok_or(AppError::NotFound)?;
    sha256_under(root, path).await
}

async fn sha256_under(root: &Path, path: String) -> Result<Json<ShaResponse>, AppError> {
    let file = resolve_under_root(root, &path).ok_or(AppError::NotFound)?;
    let bytes = tokio::fs::read(&file)
        .await
        .map_err(|_| AppError::NotFound)?;
    let digest = Sha256::digest(&bytes);
    let sha256 = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Json(ShaResponse {
        algorithm: "SHA-256",
        path,
        sha256,
        bytes: bytes.len(),
    }))
}

async fn submit_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Redirect, AppError> {
    submit_upload_to(&state.app_jobs, RootScope::App, &mut multipart).await
}

async fn submit_common_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Redirect, AppError> {
    let jobs = state.common_jobs.as_ref().ok_or(AppError::NotFound)?;
    submit_upload_to(jobs, RootScope::Common, &mut multipart).await
}

async fn submit_upload_to(
    jobs: &JobQueue,
    scope: RootScope,
    multipart: &mut Multipart,
) -> Result<Redirect, AppError> {
    let mut directory = String::new();
    let mut filename = None;
    let mut bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::BadRequest(format!("invalid multipart body: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "directory" => {
                directory = field.text().await.map_err(|error| {
                    AppError::BadRequest(format!("invalid target directory field: {error}"))
                })?;
            }
            "file" => {
                filename = field.file_name().map(|value| value.to_string());
                bytes = Some(field.bytes().await.map_err(|error| {
                    AppError::BadRequest(format!("failed to read uploaded file: {error}"))
                })?);
            }
            _ => {}
        }
    }

    let filename =
        filename.ok_or_else(|| AppError::BadRequest("missing uploaded file name".to_string()))?;
    let bytes =
        bytes.ok_or_else(|| AppError::BadRequest("missing uploaded file bytes".to_string()))?;

    let job = jobs
        .enqueue_upload(directory, filename, bytes.to_vec())
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;

    Ok(Redirect::to(&format!("{}/{}", scope.jobs_base(), job.id)))
}

pub(crate) enum AppError {
    NotFound,
    BadRequest(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
        }
    }
}
