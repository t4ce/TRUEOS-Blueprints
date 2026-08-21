# S001 Basic Feature

## Goal

Define the basic feature scope for `TRUEOS-file-system` and capture the next implementation priorities for the current stage.

## Current State

- Static file serving works for a configurable root directory.
- HTML tree browsing is available for the root and subdirectories.
- Basic path traversal protection is already implemented.
- The project is positioned as a lightweight development-oriented server.
- There is no documented automated test coverage yet.

## Milestone 1: Stabilize Core Behavior

- Add unit tests for path normalization and traversal rejection.
- Add integration tests for `/`, `/tree`, `/tree/*path`, and direct file access.
- Verify behavior for missing files, missing directories, and invalid paths.
- Confirm hidden files and directories are consistently excluded from tree output.

## Milestone 2: Improve Browser Experience

- Add breadcrumb navigation for nested directories.
- Show basic file metadata such as size and modified time.
- Support predictable sorting for files and directories.
- Refine the HTML layout for smaller screens.

## Milestone 3: Improve Runtime Configuration

- Make host and port configurable from the CLI.
- Add clearer startup logs for the resolved root path and bind address.
- Document expected behavior for directory indexes and static file fallback.
- Consider a safer default bind address for local-only development.

## Milestone 4: Prepare for Broader Usage

- Add structured logging or request tracing.
- Define basic performance expectations for large directory trees.
- Review error pages and failure messaging.
- Document deployment caveats and security boundaries more clearly.

## Risks

- Large directories may affect tree rendering latency.
- Route precedence can create edge cases between tree pages and static files.
- Cross-platform filesystem behavior may differ for symlinks and permissions.
- The current server model is not suitable for untrusted network exposure.

## Done Criteria

- Core routes are covered by automated tests.
- Main configuration options are documented and predictable.
- File tree navigation remains correct for nested directories.
- Known security boundaries are explicitly documented.

## Notes

- Update this document as milestones are completed or reprioritized.
- Link future design notes or implementation tasks from this file.
