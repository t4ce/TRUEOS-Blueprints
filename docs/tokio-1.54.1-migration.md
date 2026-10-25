# Tokio 1.54.1 migration

Tokio moves from the TRUEOS 1.52.3 vendor to a 1.54.1 vendor while retaining the
filesystem broker, descriptor conversion, networking, runtime-builder, and
shared-carrier scheduling changes. The unmodified `tokio-macros` and `tokio-util`
vendors and their Cargo/packer patch registrations are removed. Macro expansion
uses registry 2.7.2 or later in the 2.7 line; the updated utility requirements
use registry 0.7.20.

The upstream macro release uses Syn 3 on the host. The upstream utility codec
feature also enables its Unix libc dependency. Fresh target-specific Cargo
graphs are checked for the intended sources and versions, including host
dependencies and accidental parallel Tokio versions.

The native timestamp setter API introduced upstream has an explicit capability
boundary: broker-backed snapshot files return `ErrorKind::Unsupported` without
changing the file, while descriptor-backed files forward to the existing std
backend. This preserves std's result and OS errno. The filesystem bridge probe
checks both paths and verifies that failed setters preserve contents, identity,
and committed revision.

Dependency pins and vendor paths move together in the Blueprint SDK, apps,
probes, generated manifests, and external catalog consumers. Voxy's old macro
vendor override is removed as well. Reticulum and Superseedr are nested
repositories; their manifest changes must accompany the parent checkout's
changes. External manifests in Voxy, Rebels, Yazi, Velory, and Codex are part of
the migration.

## Packer fixes exposed by the upgrade

Nested workspace members previously copied the owning workspace lockfile but
looked only beside the member manifest when checking source-overlay version
mismatches. Codex therefore retained locked Tokio 1.52.3 while the SDK required
1.54.1. Both operations now share the same lockfile selection.

The version-alignment preflight also inspected unrelated members returned in
workspace-wide Cargo metadata. It now checks the requested package's dependency
closure, retaining transitive host/build constraints. Regression coverage checks
that an unrelated member's exact pin cannot block the selected app and that an
incompatible transitive build dependency still does block it.

## Validation

The standalone Tokio host probe passed two tests covering filesystem operations,
timestamp setters, owned descriptor conversion, and multithreaded spawning and
yielding. All 12 packer staging regressions passed.

Full catalog packing and fresh target-specific dependency-graph verification
are in progress. Results below will be completed after the final pack.

Validation artifacts and the pre-migration `dist` backup are under
`/tmp/trueos-tokio-1541-validation/`. The host probe is under
`/tmp/trueos-tokio1541-host-probe/`.

```sh
# Local artifact validation; publication is deliberately skipped.
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp

# The normal bulk command excludes built-ins and rustc-min.
# Build the built-ins by their names in buildins.json separately.

TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp \
  tokio_fs_bridge tokio_fs tokio_rt tokio_mrt tokio_stack \
  tokio_stop tokio_quit tokio_kill
```

The normal network-enabled pack is used. QuickJS's current build script checks
`CARGO_NET_OFFLINE` before checking its cached source checkout, so setting that
environment variable prevents the cached native-engine build.

No public catalog has been modified and no physical-rig result is claimed.
Fresh runtime verification must install the newly built archives and compare
the launched `TRUEOS_APP_SHA256` with their local hashes; a bare AppDB-name launch
or HTTP filesystem upload alone can otherwise exercise an older artifact.
