# TRUEOS Share

A standalone read/write WebDAV test Blueprint using `dav-server` 0.11.0 and
Hyper HTTP/1. It serves its own persistent `TRUEOS_APP_FS_ROOT` (normally
`apps/share`) on TCP port **4918**. It does not require `websys` or the kernel's
port-80 filesystem service.

This first version deliberately has **no authentication and no TLS**. Every
client that can reach the port can read and modify the exported directory.

## Build for TRUEOS

From the TRUEOS-Blueprints repository:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp share
```

This produces `dist/share.bp` without publishing or launching it. The app is
registered as a built-in in `buildins.json`, with its sources in `apps/share`.
The default TRUEOS startup manifest launches it headlessly from the embedded
AppDB on cold boot, without fetching it from the online app catalog.
It uses the normal VMX Blueprint execution path, without a guest Linux system.
The root is created when the server starts. Files live in TRUEOSFS independently
of the server process.

## Connect from Linux

After launching the Blueprint on TRUEOS, enter this location in GNOME Files:

```text
dav://TRUEOS-IP:4918/
```

Or mount and list it using GIO (requires the GVfs WebDAV backend):

```sh
gio mount dav://TRUEOS-IP:4918/
gio list dav://TRUEOS-IP:4918/
```

For an ordinary filesystem mount, use the existing `davfs2` client with
`http://TRUEOS-IP:4918/` as its server URL. A browser can also browse and download
files from that HTTP URL.

## Local development and tests

The host manifest builds the same sources against upstream desktop libraries;
the parent workspace's modified Hyper/http-body-util currently only build on
TRUEOS.

```sh
cargo test --manifest-path apps/share/host/Cargo.toml
TRUEOS_SHARE_BIND=127.0.0.1:4918 cargo run \
  --manifest-path apps/share/host/Cargo.toml -- /tmp/share-test
```

`[ROOT]` selects a different serve directory. Without an argument the server uses
`TRUEOS_APP_FS_ROOT`, or `share-data` on a development host. `TRUEOS_SHARE_BIND`
overrides the default `0.0.0.0:4918` listener.

## Supported operations and current limits

- OPTIONS, PROPFIND, GET/HEAD with byte ranges, PUT, MKCOL, COPY, MOVE, DELETE.
- LOCK/UNLOCK use real in-memory WebDAV locks, lost when the server restarts.
- HTTP directory listings are enabled. Symlinks are hidden and rejected by the
  backend; the share root itself cannot be deleted or moved through WebDAV.
- Successful PUT waits for file flush and storage synchronization.
- Files are limited to **4 GiB** (4,294,967,296 bytes). TRUEOS's current Tokio
  file bridge buffers whole files. Four HTTP connections can run at once.
  The kernel must include the matching 4 GiB async-filesystem read/write caps;
  earlier kernels restrict buffered writes to 16 MiB and reads to 1 GiB.
- Native metadata currently exposes size and type, without timestamps or ETags.
- TRUEOSFS moves inherit its copy/delete rename behavior. Atomic replacement,
  crash-safe saves and concurrent editing have not been established on hardware.
  Interrupted uploads may leave partial files; this version does not stage PUT
  into an atomic temporary-file replacement.

The automated host tests cover file operations, range downloads, overwrite,
storage visibility before PUT returns, locks, path confinement, size limits and
the actual HTTP socket driver. Building the Blueprint establishes target
compatibility; runtime behavior still needs a test on TRUEOS hardware.
