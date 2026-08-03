<p align="center">
  <img width="300" height="300" alt="Gemini_Generated_Image_r9d18er9d18er9d1" src="https://github.com/user-attachments/assets/06f129f1-a6b0-4885-a6f1-f0d2c7b6a569" />
</p>

# SceneDB

GPU-native ECS and spatial database for game engines, with a headless TRUEOS
vVideoMem proof.

SceneDB keeps entity data in cache-friendly SoA pages on the CPU side, syncs
only what changed to GPU buffers each frame, and gives you stable handles that
don't dangle when things get compacted. AVX2/NEON spatial queries, a streaming
grid for world cells, and a compile-time frame phase machine that prevents you
from mutating stuff during the readback phase.

The upstream host path still supports its optional WGPU mirror. The Blueprint
path is deliberately different: SceneDB's Pod page and liveness allocation
seams allocate VM-owned `VVideoMem`, so the guest CPU mapping and the tenant
PPGTT mapping terminate at the same DDR pages. Dirty ranges perform cache
publication, not `Queue::write_buffer` copies.

The standalone `scenedb-vvideomem` binary creates a 256-row cell and compares
the trusted Xe-LP AABB dispatch against SceneDB's CPU oracle for all-live,
dead-row, sparse, touching-face, empty, and NaN cases. It then changes two
`f32` words in place, flushes only those words, and requires the second UHD770
result to change while `copied_upload_bytes` remains zero. The report also
requires a real GuC serial and verifies that CPU and PPGTT translations resolve
to identical pages.

After that correctness gate, the same headless binary extracts the useful part
of the upstream stress TUI without compiling its terminal stack. A 1,024-row
SceneDB worker runs 128 CPU-checked AABB dispatches through TRUEOS's
`pthread_create` background-AP service lane while the Hull thread fills the
guest's reported vGPU memory quota to at least 94%, verifies an over-quota
allocation is rejected, and performs 64 allocation/unmap/remap cycles. Each
cycle touches and flushes both ends of a page-backed region. The stress PASS
line requires monotonically increasing physical completion serials, identical
CPU/GPU positional tokens, mapping identity, zero copied-upload bytes, and full
buffer retirement. This is finite and log-driven; no TUI, WGPU, telemetry, or
random-number API is used by the stress path.

Build the Blueprint from the repository root with:

```sh
cargo bp apps/SceneDB
```

The package's normal graph disables WGPU and telemetry and excludes examples,
benches, integration tests, dashboard/TUI code, and all dev dependencies.

The source tree contains:

- **pulsar_scenedb** — the core library. Archetype ECS, paged storage layer,
  SIMD culling, GPU mirroring (feature-gated), streaming grid, asset stores.
- **pulsar_scenedb_derive** — the upstream derive crate, retained as source but
  excluded from the headless Blueprint workspace.

The upstream tests, benches, and TUI sources remain available for normal host
development; none are compiled into this Blueprint.

Licensed under MIT OR Apache-2.0.
