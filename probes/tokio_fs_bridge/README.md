# Tokio standard-file bridge probe

Pack with `cargo bpp tokio_fs_bridge`. The probe checks that converting an open
standard file to Tokio preserves its cursor and append access, that read-only
handles reject writes, and that the blocking file returned by `into_std` can be
owned by a reader. It also checks snapshot-file seeks beyond EOF and writes.
The result is saved to `/common/tokio-fs-bridge-report.json`.

The 2026-10-13 devrig run caught the kernel standard-file shim ignoring O_APPEND
on writes. After the kernel fix and its normal rebuild/boot, the same probe
passed all checks. Downloaded files contained the expected appended and rewritten
bytes. `verification.json` records the loaded artifact hash and results. The
recorded test artifact uses the supported raw Blueprint payload format.

The corresponding Codex pack cleared all four codex-rollout errors and reached
four errors in codex-exec-server plus one in codex-message-history: standard file
ownership, metadata, OpenOptions conversion, and file-descriptor borrowing.

## Metadata mapping

| Metadata | TRUEOS mapping |
| --- | --- |
| File/directory kind | Native TRUEOSFS node kind |
| Symlink | Always false; TRUEOSFS has no symlinks |
| Size | Current file length |
| File identity | Persistent root-scoped ID; retained by overwrite, append, and rename |
| Copy or delete/recreate | New ID |
| Legacy file identity | Existing entry LBA, carried into subsequent headers |
| Standard metadata | `st_ino` = file ID; `st_dev` = mounted root ID |
| Tokio metadata | `file_id()` and `root_id()` report the same values |
| Creation/modification times | Unsupported through Tokio; Codex reports 0 |
| Change-time integer | Committed record LBA split into `ctime` seconds and `ctime_nsec`; logical revision, not calendar time |
| Snapshot validation | Root ID, file ID, length, and committed revision; equal-length rewrites invalidate reuse |
| Permissions | Standard `std::fs::Permissions` type; validated permission-copy calls follow the native development no-op policy |

The additive `trueos_cabi_async_fs_metadata_start_v1` returns 24 little-endian
bytes: kind `u32`, length `u64`, file ID `u64`, mounted root ID `u32`.
Metadata v2 retains those 24 bytes and appends a `u64` committed record revision
for a total of 32 bytes. Tokio uses v2. The legacy stat, typed-stat, and metadata
v1 wire formats remain unchanged. Root directories
and files awaiting their first commit have no file ID. Mounted root IDs are
runtime device identifiers, not persistent volume UUIDs.

The optional `TOID` extension occupies header bytes 112–124. The updated kernel
reads existing zero-tailed records and preserves their identity when writing.
An older kernel does not understand records carrying this extension.

The expanded probe compares standard-file and Tokio identities, checks identity
after append and overwrite, checks that recreation changes it, and verifies
kind, unsupported calendar timestamps, standard/Tokio revision agreement, and
revision changes on equal-length overwrites. The previous `verification.json` covers the
bridge checks before this metadata extension; the expanded hardware test needs
a rebuild/boot of the updated kernel.

Permission copying uses the ordinary std/Tokio I/O type. It neither writes mode
bits into TRUEOSFS nor interprets them as access keys. Descriptor-backed handles
use native `fchmod`; snapshot handles and paths validate the target before
accepting the call. Missing paths still return their normal I/O error. Future
read/write authorization belongs in filesystem I/O against the caller's keys.
The expanded probe checks both handle forms and path permission copying without
changing contents, file identity, or committed revision.

## Tokio 1.54 timestamp setters

The probe checks `File::set_times` and `File::set_modified` on a native Tokio
snapshot handle. Both must return `ErrorKind::Unsupported`; the native setter
must not manufacture an OS errno or change contents, file identity, or committed
revision. This makes the absence of a writable timestamp CABI explicit.

For a descriptor-backed `File::from_std` on TRUEOS, the probe compares the Tokio
setter's error kind and raw OS errno to a direct `std::fs::File::set_times` call.
The current kernel `futimens` shim returns ENOSYS. Matching the raw errno verifies
that the original descriptor reaches std's backend instead of the snapshot
fallback. The PASS report records `timestamp_boundary` and
`descriptor_timestamp_forwarding` alongside the existing bridge checks.
