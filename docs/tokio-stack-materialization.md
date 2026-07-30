# Tokio stack materialization

`tokio_stack` is the integrated Blueprint witness for the Tokio ecosystem
layout used by TRUEOS. It exercises a generated unary gRPC service and client
over a loopback TCP connection, with a Tower service, direct Bytes operations,
Tokio tasks and spans, and the TRUEOS Tracing subscriber.

## Pinned surface

| Surface | Pin | Materialization boundary |
| --- | ---: | --- |
| Rust `core` / `alloc` / `std` | TRUEOS nightly 2026-07-10 | Language and app-specific code is linked and garbage-collected. Allocation, clocks, thread identity, WLS, I/O, and abort behavior terminate at kernel imports. Symbolic panic backtraces are excluded for TRUEOS/zkvm. |
| Mio | 1.2.0 | Selector, wake, and TCP operations terminate at `trueos_mio_*`. |
| Tokio runtime | 1.52.3 | Polling, clocks, wakeups, task scheduling, WLS, and networking use the TRUEOS Tokio/Mio platform adapters. |
| Bytes | 1.11.1 | App-owned buffer shape and referenced operations remain local; storage uses the kernel allocator ABI. This avoids exporting Rust object layout across CABI. |
| Hyper | 1.9.0 | HTTP state remains application code; Tokio I/O, time, DNS, and TCP terminate at the kernel-backed adapters. |
| Tower | 0.5.3 | The selected service/layer combinators remain local and are section-GC'd; their runtime work uses Tokio's adapter. |
| Axum | 0.8.9 | Routes and handlers remain application policy. Axum materializes through Tower and Hyper; existing Axum applications retain this path. Tonic's server router also traverses it. |
| Tonic | 0.14.6 | Generated Prost messages and gRPC/HTTP2 state remain application code. Channel, timeout, server, and socket paths materialize through Hyper, Tower, Tokio, and Mio. |
| Tracing | 0.1.44 | Static callsites remain local. The reusable `trueos::trace::KernelSubscriber` sends span creation and structured events directly to the kernel log ABI; `tracing-subscriber` is not required. |

All listed versions are exact Cargo constraints or canonical vendored paths,
and the generated lockfile pins their transitive graph.

## Evidence

Build and publish the integrated witness with:

```sh
cargo run -p trueos-blueprint -- --probes tokio_stack
```

The packer must report `compatible=1`. The resulting module must retain the
expected `trueos_cabi_*`, `trueos_tokio_platform_*`, and `trueos_mio_*`
undefined imports, with no `backtrace_rs`, `gimli`, `addr2line`,
`rustc_demangle`, or `miniz_oxide` symbols.

The 2026-07-30 v3 evidence was:

| Witness | Raw relocatable | Packed Blueprint | Kernel ABI imports |
| --- | ---: | ---: | ---: |
| `tokio_stack` | 4,228,720 bytes | 581,864 bytes | 10 |
| `tokio_rt` | 879,936 bytes | 113,276 bytes | 9 |
| `framework_stack` | 657,104 bytes | 102,735 bytes | 8 |

For comparison, the preceding std layout packed `tokio_stack` at 719,667
bytes, `tokio_rt` at 224,547 bytes, and `framework_stack` at 207,076 bytes.

Runtime success is the terminal log record:

```text
tokio_stack: done runtime=mio,tokio bytes=direct hyper=tcp tower=service tracing=kernel-log tonic=grpc
```

This contract deliberately does not claim that application schemas, route
policy, or Rust buffer object layouts are kernel ABI. It claims that the full
stack is accepted, laid out, garbage-collected, ABI-guarded, and packaged by
the Blueprint system, and that reusable runtime/transport/logging mechanisms
terminate at the TRUEOS kernel surface.
