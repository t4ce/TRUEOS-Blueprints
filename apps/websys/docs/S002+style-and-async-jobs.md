# S002 Style And Async Jobs

## Goal

Define the next feature scope for UI consistency and asynchronous file operation handling in `TRUEOS-file-system`.

## Feature List

| ID | Feature | Description |
|----|---------|-------------|
| F001 | Shared CSS file | Create a dedicated CSS file that centralizes all colors, spacing, and size tokens. Reuse this file across the system so the file system UI keeps a consistent visual style. |
| F002 | Asynchronous job queue | Introduce an asynchronous job queue to handle file-system-related operations such as move, delete, upload, download, and other future file tasks. |
| F003 | Backend task execution | When the page sends an operation request such as moving a file, the backend system service should accept the request, create or dispatch the job, and execute the task asynchronously. |

## Scope Details

### F001 Shared CSS File

- Extract existing page styles into a dedicated reusable CSS asset.
- Replace hard-coded colors, spacing, and size values with shared style tokens.
- Ensure all current and future file system pages can consume the same stylesheet.

### F002 Asynchronous Job Queue

- Define a job model for file operations.
- Support queueing, execution, and status tracking for long-running or state-changing tasks.
- Reserve extension space for additional file operation types beyond the initial set.

### F003 Backend Task Execution

- Accept operation requests from the page layer through a clear backend entry point.
- Hand off execution to the job queue instead of blocking the request path.
- Return task acceptance or task status information so the frontend can reflect progress.

## Acceptance Criteria

- A standalone CSS file exists and is reused by the file system UI.
- File operation requests are processed through an asynchronous queue instead of direct inline handling.
- Move, delete, upload, and download operations can be represented as jobs.
- The backend service can receive a page request and trigger asynchronous task execution successfully.

## Completion Status

- Completed: shared stylesheet extracted to `assets/ui.css` and served through `/ui/style.css`.
- Completed: tree page and job pages now reuse the same stylesheet.
- Completed: move, delete, upload, and download preparation now enter a background job queue.
- Completed: page-side forms submit requests to backend endpoints, and the worker executes them asynchronously.
- Completed: a public Rust API is exposed through `src/lib.rs`, `JobQueue`, and `JobRequest` for direct programmatic use.
- Verified: the staged download job flow was smoke-tested successfully against the local example data.

## Notes

- This scope focuses on structure and capability definition, not full implementation details.
- Refine API shape, queue persistence strategy, and status reporting rules during implementation.
