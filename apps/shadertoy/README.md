# ShaderToy visual Blueprint

The Blueprint owns the five runtime shaders in `assets/`. Each shader directory
contains the raw GLSL (`input.glsl`), generated C++ (`kernel.clcpp`), Zebin,
SPIR-V, bake manifest, and ABI contract. The corresponding `.stpkg` includes
all six files and is embedded in the Blueprint. `build.rs` rejects stale packages.

The app registers these packages through bounded, window-owned transfers before
rendering. TRUEOS authenticates the complete package with its trusted SHA-256,
then checks the Zebin and SPIR-V hashes, PCI device/revision, ABI and ELF layout.
Only the approved executable is copied into kernel-owned DMA memory and mapped
into the dispatch PPGTT. GPU addresses, surfaces and dispatch geometry remain
kernel-owned. The existing trust scheme is an exact-byte hash allowlist, not a
public-key shader signing/update channel. Source and provenance now receive
package hash coverage too.

The app opens a UI4 visual frame at 30 Hz. Use Left/Right or F1–F5 to switch
shaders; Escape closes the app. Per-frame calls still carry the existing 64-byte
uniform block. Registering an incomplete, modified or unknown package cannot
make a new shader executable. There is no embedded/filesystem shader fallback.

For a local package build from TRUEOS-Blueprints:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp shadertoy
```

Rebuild the kernel and Blueprint together for this transport change. An older
Blueprint does not register packages and cannot render on the new kernel.

From TRUEOS, regenerate and reproducibly bake the five shaders with the existing
locked compiler toolchain:

```sh
make intel-gpu-bake-shadertoy-cpp
python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py
```

`TRUEOS_BLUEPRINTS_ROOT` can select a different Blueprint checkout. The bake
script writes payloads into these assets and keeps only contracts and package
hash/length metadata in TRUEOS. `package_blueprint.py --check` verifies packages
without modifying them; `--update-trust` refreshes the kernel package hashes
following review. A new candidate still needs explicit catalog admission.
