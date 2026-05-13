# TRUEOS Blueprint SDK Manifest

Status: design manifest for the next SDK split

## Intent

Separate the blueprint toolchain into two explicit layers:

1. Blueprint app packer
2. TRUEOS sysroot or overlay SDK

The packer owns app staging and bundle creation.
The SDK owns platform semantics.

This keeps app packaging simple while preserving a direct path for the usual std hotfixing work.

## Ownership Boundary

### Blueprint App Packer

The packer is responsible for:

- selecting the app entry artifact
- staging the app source tree
- collecting assets and metadata
- choosing the blueprint output name
- ABI stamping and runtime declaration
- invoking cargo for the app build
- creating the final `.bp` bundle
- publishing bundles into the apps destination

The packer must not become the place where TRUEOS platform behavior is invented.
It is a build-and-bundle layer, not the long-term home of target cfg policy.

Current implementation anchors already in this repo:

- cargo artifact staging and workdir setup in `src/main.rs`
- source overlay staging in `src/main.rs`
- final blueprint writing in `src/main.rs`
- dist publishing in `src/main.rs`

### TRUEOS Sysroot Or Overlay SDK

The SDK is responsible for:

- the TRUEOS target spec
- `build-std` policy
- patched `std` and `core` behavior for TRUEOS
- `libc` shims and ABI glue
- cfg policy such as whether TRUEOS advertises `unix`
- linker behavior and target rustflags
- Rust source overlays used to hotfix platform crates
- compatibility patches for vendored or overlay crates

This is the right layer for time, fs, env, metadata, socket, and future std reroutes.
It is also the correct place for temporary hotfixes that should apply to every blueprint app.

Current implementation anchors already in this repo:

- target spec discovery in `src/main.rs`
- source overlay patch collection in `src/main.rs`
- staged lock overlay updates in `src/main.rs`
- std reroute design notes in `STD_REROUTE_MEMO.md`

## Rule

If a change exists because TRUEOS differs from upstream Rust platform assumptions, it belongs in the SDK layer.

If a change exists because a blueprint app needs to be staged, described, packed, or published, it belongs in the packer layer.

## Why This Split

This split gives the cleanest maintenance path for standard library hotfixing:

- one target spec surface instead of per-app drift
- one cfg policy instead of crate-local hacks
- one place to patch `std`, `libc`, Tokio shims, or vendored crates
- one place to reason about app root semantics, fs reroutes, and ABI behavior
- fewer vendored-crate one-offs inside individual blueprint apps

It also keeps the blueprint packer understandable. The packer can stay focused on transforming an app tree into a distributable bundle.

## Compatibility Phases

The SDK should expose its platform work as an ordered compatibility ladder instead of a flat pile of patches:

1. target and cfg phase
2. libc surface phase
3. std contract phase
4. tokio platform phase

The important rule is that Tokio is not the first compatibility layer.
Tokio comes after the std contract is mapped well enough that async crates are no longer compensating for missing baseline OS semantics.

That gives TRUEOS a clear story:

- target phase decides `os`, `env`, `target-family`, and linker policy
- libc phase supplies the Unix and newlib contract that upstream `std` expects to see
- std phase reroutes stable platform behavior such as time, fs, env, and thread-adjacent hooks into TRUEOS-owned implementations
- Tokio phase maps a relatively large async/runtime subset onto TRUEOS platform services once the std layer is already coherent

This keeps Tokio as a deliberate SDK acceleration layer rather than a substitute for missing std ownership.
It also lets us adopt broad Tokio-backed behavior only where TRUEOS primitives are already proven.

## Proposed Layout

```text
crates/TRUEOS-Blueprints/
  src/
    main.rs                  # blueprint app packer
  sdk/
    targets/
      trueos-blueprint.json  # canonical blueprint target spec
    overlays/
      libc/
      std/
      tokio/
    manifests/
      sdk.toml               # SDK release/ABI/cfg declaration
  dist/
  examples and apps...
```

The important idea is not the exact directory name. The important idea is that the canonical target spec and overlay policy move under an explicit SDK root.

## Proposed Commands

### Packer Commands

- `cargo bp build <app>`
- `cargo bp pack <app>`
- `cargo bp publish <app>`

### SDK Commands

- `cargo bp sdk doctor`
- `cargo bp sdk print-target`
- `cargo bp sdk overlay list`
- `cargo bp sdk std-status`

The packer commands operate on apps.
The SDK commands operate on platform state.

## Bundle Manifest Shape

Each blueprint bundle should contain a small manifest that describes the runtime contract, while the platform policy still lives in the SDK.

Example:

```toml
manifest-version = 1

[app]
id = "localcoder"
name = "Local Coder"
entry = "localcoder"

[bundle]
format = "bp"
layout = "single-artifact-plus-assets"

[runtime]
abi = "trueos-blueprint-v1"
sdk = "trueos-blueprint-sdk-v1"
mode = "std"
cwd = "app-root"

[assets]
root = "assets"

[capabilities]
fs = true
net = true
time = true
ui = false
```

This bundle manifest declares what the app expects.
It does not redefine target cfg behavior. That remains SDK-owned.

## SDK Manifest Shape

The SDK itself should also have an explicit manifest so target policy is not hidden across ad hoc config files.

Example:

```toml
manifest-version = 1

[sdk]
id = "trueos-blueprint-sdk"
version = "0.1.0"

[target]
spec = "targets/trueos-blueprint.json"
os = "trueos"
families = ["unix"]

[build_std]
std = true
panic = "abort"
components = ["core", "compiler_builtins", "alloc", "std", "panic_abort"]

[overlays]
enabled = ["libc"]
optional = ["std", "tokio"]

[overlay_policy]
apply_order = ["libc", "std", "tokio"]
tokio_requires = ["std"]

[policy]
app_root_cwd = true
reroute_std_fs = true
reroute_std_time = true
tokio_after_std = true
```

This makes cfg policy and std hotfix policy inspectable.
It also makes the Tokio step explicit instead of leaving it as an unstructured optional overlay.

## Tokio SDK Step

After the std map is in place, the next clear SDK step is a Tokio platform layer.
That layer should own mappings like:

- monotonic time and sleep backed by TRUEOS time services
- runtime park and wake behavior backed by TRUEOS poll or host-yield hooks
- blocking pool fallback behavior where TRUEOS does not yet have full condvar semantics
- async networking glue where TRUEOS already has a proven socket or selector path

This is a valid large-subset strategy because Tokio already centralizes a lot of runtime behavior behind a smaller set of platform seams.
The SDK can use that to unlock a broad async ecosystem without pushing app crates into local workarounds.

The constraint remains the same: Tokio mapping should extend the SDK after std, not replace the std layer.
If an issue exists because upstream `std` assumes Unix or libc behavior, fix that in the std or libc phase first.
If an issue exists because async runtime machinery needs a TRUEOS host hook, that belongs in the Tokio SDK phase.

## Migration Path

1. Keep the current packer behavior intact.
2. Move the canonical blueprint target spec under an explicit SDK root.
3. Move source overlay policy under that SDK root.
4. Add a small SDK manifest that declares cfg and std behavior.
5. Make the std-to-Tokio compatibility order explicit in that SDK manifest.
6. Emit a bundle manifest into each `.bp` artifact.
7. Stop solving platform mismatches inside per-app vendored crates when the issue is really SDK-owned.

## Immediate Practical Guidance

Until the split is fully implemented:

- treat per-app target specs such as `localcoder/trueos.json` as SDK-owned inputs
- treat source overlays such as `libc` patches as SDK policy, not app-local hacks
- prefer fixing TRUEOS platform assumptions in the blueprint SDK path before vendoring leaf crates inside apps

## Decision Summary

The blueprint app packer should own staging, manifesting, assets, ABI stamping, and bundle creation.

The TRUEOS sysroot or overlay SDK should own the target spec, patched std, libc shims, cfg policy, linker behavior, and Rust source overlays.

That is the architectural split that best supports normal std hotfixing without turning every blueprint build issue into an app-local workaround.