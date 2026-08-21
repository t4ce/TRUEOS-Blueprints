# TRUEOS File System

A lightweight file manager and static file server written in Rust. It serves files from configurable app and common roots and provides an HTML file-tree browser for navigation.

Built with [Axum](https://github.com/tokio-rs/axum) and the native TrueOS asynchronous filesystem/archive APIs.

## Features

- **Static file serving** — download or open files under the root directory
- **HTML file tree** — browse the full directory hierarchy in the browser
- **TRUEOSFS record keys** — each file shows its on-disk `🔑 FFA` or provider/key-handle badge
- **SHA-256 on demand** — each file has an asynchronous SHA button with an inline loading state and result
- **Archive mode** — select multiple files/directories and create a native `.7z` in an explicitly chosen relative directory (an empty/root path is rejected)
- **Download mode** — select multiple files/directories and prepare one downloadable `.7z`
- **7z extraction** — unpack an archive into a selected relative directory
- **Shared UI stylesheet** — all visual tokens and component styles are served from one reusable CSS file
- **Asynchronous job queue** — move, delete, upload, archive, extract, and download preparation run through a background worker
- **Job status pages** — every queued operation gets a dedicated status page and result link when applicable
- **Configurable root** — pass any directory as the serve root via CLI
- **Path safety** — requests are canonicalized and must stay within the root (blocks `..` traversal)
- **Dark mode** — UI follows system `prefers-color-scheme`

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024)
- `7z` on `PATH` for host development; TrueOS builds use the integrated `v::varchive` API

## Quick start

```bash
# Clone and enter the repo
git clone https://github.com/iamwjun/TRUEOS-file-system.git
cd TRUEOS-file-system

# Run with the bundled example directory
cargo run -- example
```

Then open:

| URL | Description |
|-----|-------------|
| http://127.0.0.1:54321/ | File tree (root) |
| http://127.0.0.1:54321/tree | Same as `/` |
| http://127.0.0.1:54321/tree/subdir | File tree for a subdirectory |
| http://127.0.0.1:54321/jobs | Job queue overview page |
| http://127.0.0.1:54321/healthz | Lightweight server health check |
| http://127.0.0.1:54321/ui/style.css | Shared CSS asset |
| http://127.0.0.1:54321/api/sha256/file.txt | SHA-256 JSON for a file |
| http://127.0.0.1:54321/file.txt | Direct file access |

The server listens on **port `54321`** (`0.0.0.0`). Set `TRUEOS_APP_FS_PORT` to override it for host development.

## Usage

```bash
# Default root: current directory
cargo run

# Custom root directory
cargo run -- /path/to/files

# Release build
cargo build --release
./target/release/file-system /path/to/files
```

### CLI

| Argument | Default | Description |
|----------|---------|-------------|
| `[ROOT]` | `.` | Directory to serve (must exist and be a folder) |

## Rust API

The crate now exposes a reusable Rust API as `file_system`, so callers can submit file jobs without going through HTTP.

```rust
use std::time::Duration;

use file_system::{JobQueue, JobRequest, JobStatus};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let queue = JobQueue::new("example");

    let upload = queue
        .enqueue(JobRequest::upload(
            "demo-data/uploads",
            "notes.txt",
            b"hello from rust".to_vec(),
        )?)
        .await?;

    let upload = queue
        .wait_for_terminal(upload.id, Duration::from_millis(50))
        .await
        .expect("job should exist");

    if upload.status == JobStatus::Succeeded {
        println!("upload result: {:?}", upload.result_path);
    }

    let move_job = queue
        .enqueue(JobRequest::move_path(
            "demo-data/uploads/notes.txt",
            "demo-data/archive/notes.txt",
        )?)
        .await?;

    println!("queued move job {}", move_job.id);
    Ok(())
}
```

Convenience methods are also available:

- `JobQueue::enqueue_move`
- `JobQueue::enqueue_delete`
- `JobQueue::enqueue_upload`
- `JobQueue::enqueue_download`
- `JobQueue::enqueue_archive`
- `JobQueue::enqueue_extract`
- `JobQueue::wait_for_terminal`

For a complete program, see `cargo run --example rust_api`.

## Tile Debug Style Object

The image-derived tile debug frame is also available as a Rust style object.

```rust
use file_system::TRUEOS_TILE_DEBUG_FRAME_0;

fn main() {
    let frame = TRUEOS_TILE_DEBUG_FRAME_0;
    let cell = frame.cell(5, 5).unwrap();

    assert_eq!(cell.label(), "R5C5");
    println!("{}", cell.fill.to_hex_rgb());
    println!("{}", frame.to_css_custom_properties("trueos-tile"));
}
```

This object is based on the 12x6 debug image under `docs/image.png`. For a runnable example, use `cargo run --example tile_debug_style`.

## Routes

| Method | Path | Handler |
|--------|------|---------|
| `GET` | `/`, `/tree` | HTML file tree for the root |
| `GET` | `/tree/*path` | HTML file tree for a subdirectory |
| `GET` | `/ui/style.css` | Shared stylesheet for the UI |
| `GET` | `/healthz`, `/api/healthz` | Lightweight connectivity check |
| `GET` | `/jobs` | HTML job queue overview |
| `GET` | `/jobs/:id` | HTML job detail and status page |
| `POST` | `/jobs/move` | Enqueue a move job |
| `POST` | `/jobs/delete` | Enqueue a delete job |
| `POST` | `/jobs/upload` | Enqueue an upload job |
| `POST` | `/jobs/download` | Enqueue a staged download job |
| `POST` | `/jobs/archive` | Archive a JSON-encoded selection into a `.7z` |
| `POST` | `/jobs/download-selection` | Archive a JSON-encoded selection for download |
| `POST` | `/jobs/extract` | Extract a `.7z` into a relative directory |
| `GET` | `/api/sha256/*path` | Return a file's SHA-256 and byte length as JSON |
| `GET` | `/*path` | Direct static file access |

The same tree, file, SHA, and job routes are available for the optional common root below `/common`.

Notes:

- Hidden files and directories (names starting with `.`) are omitted from the tree view.
- Directories without `index.html` return the tree page when accessed via `/tree/...`; direct file routes serve `index.html` when present.
- Download jobs stage a copy under `/.job-downloads/...`, which stays hidden from the tree but remains directly servable.
- Multi-file archive jobs preserve each selected path relative to the capability root and collapse redundant child selections.

## Project layout

```
TRUEOS-file-system/
├── assets/
│   └── ui.css     # Shared CSS tokens and component styles
├── Cargo.toml
├── examples/
│   ├── rust_api.rs # Programmatic Rust API example
│   └── tile_debug_style.rs # Tile debug style and CSS export example
├── src/
│   ├── lib.rs      # Public Rust API exports
│   ├── main.rs    # Server setup, routing, HTTP handlers
│   ├── jobs.rs    # Asynchronous file operation queue and worker
│   ├── tile_debug_style.rs # Image-derived Rust style objects and palette
│   ├── tree.rs    # Directory scanning, path encoding, traversal checks
│   └── html.rs    # HTML page rendering for tree and job pages
└── example/       # Sample files for local testing
```

## Development

```bash
# Build
cargo build

# Run tests (if added)
cargo test

# Format & lint
cargo fmt
cargo clippy
```

## Security

This is a **development-oriented** static server, not hardened for production:

- No authentication or access control
- Binds to all interfaces (`0.0.0.0`)
- Only serves content under the canonicalized root; path traversal via `..` is rejected

Do not expose it to untrusted networks without additional protection (reverse proxy, firewall, auth).

## License

See repository license file if present.
