# Veloren server Blueprint build attempt

This package tries the existing `veloren-server-cli` source through the TRUEOS
Blueprint packer without adding Veloren to the TRUEOS-Blueprints Cargo
workspace or changing Veloren's root `Cargo.toml`. `src/main.rs` includes the
upstream CLI as the crate root, so its console commands, web UI, and server
logic remain the same.

The first build leaves `worldgen` and `persistent_world` disabled to isolate
the basic server, console, and web-service dependency path. Enable those Cargo
features in this manifest for a later full-server attempt.

From `TRUEOS-Blueprints`, with the Veloren checkout beside it at
`../veloren`, run:

```sh
VELOREN_SOURCE_ROOT="$(realpath ../veloren)" \
CARGO_WORKSPACE_DIR="$(realpath ../veloren)" \
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 \
cargo bp apps/velosrv
```

The environment variables provide the source and compile-time workspace path
to Veloren's included modules. Publication is disabled so this is a local pack
attempt only. Build failures identify the next platform dependency that needs
an adapter; a successful `.bp` build still needs a runtime launch check before
the server can be considered supported.

## Runtime storage paths

Veloren uses `current_exe()` only as a fallback for locating userdata. Its
`VELOREN_USERDATA` runtime variable takes precedence. TRUEOS seeds this variable
with `<VM HOME>/userdata` for `velosrv`, so it is visible in `vmx_env` and uses
the existing per-instance filesystem scope. For the default instance it is
`/apps/velosrv/userdata`; named instances use their own VM home. This works with
older packed server code that still has the executable-relative fallback.

CLI settings live below `userdata/server-cli`, while server configuration and
saves live below `userdata/server` (saves use `userdata/server/saves`).
The `vmx_env` view currently displays variables; it does not edit them.

Assets are a separate directory, selected by `VELOREN_ASSETS`. The current
TRUEOS source also searches app-local `assets` and then
`/common/veloren/assets` (the latter maps to `apps/common/veloren/assets`).
The shared alias must be absolute: the asset loader canonicalizes its root
against the app's working directory before opening it, so a relative
`common/veloren/assets` would refer to a directory inside the app's own home.
That directory needs the actual Veloren asset tree. Setting a path does not
grant filesystem access outside the Blueprint's app/common scope.
