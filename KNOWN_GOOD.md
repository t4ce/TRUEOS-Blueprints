## Framework Support Crates Pulled Into The Proof

| Crate | Version | Blueprint feature | Source | Purpose |
| --- | --- | --- | --- | --- |
| `tokio` | `1.52.1` | `tokio-runtime`, `tokio-net-probe` | `../TRUEOS/vendor/tokio-1.52.1` | Runtime, tasks, time, IO, fs, optional net |
| `mio` | `1.2.0` | `tokio-net-probe` | `../TRUEOS/vendor/mio-1.2.0` | Poll/waker/socket substrate |
| `socket2` | `0.6.3` | `tokio-net-probe` | `../TRUEOS/vendor/socket2-0.6.3` | Socket construction substrate |
| `hyper` | `1.9.0` | `framework-probe` | `../TRUEOS/vendor/hyper-1.9.0` | HTTP/1 client/server types and builders |
| `tower` | `0.5.3` | `framework-probe` | crates.io | Service trait utilities and layers |
| `serde` | `1.0.228` lock | default root dep | crates.io | `alloc` JSON/data model derive support |
| `serde_json` | `1.0.149` lock | default root dep | crates.io | `alloc` JSON values and parsing |
| `bytes` | `1.11.1` lock | transitive | crates.io | byte buffers used by HTTP/Tokio crates |
| `http` | `1.4.0` lock | `framework-probe` transitive | crates.io | HTTP request/response types |
| `http-body` | `1.0.1` lock | `framework-probe` transitive | crates.io | HTTP body trait |
| `httparse` | `1.10.1` lock | `framework-probe` transitive | crates.io | HTTP/1 parsing |
| `httpdate` | `1.0.3` lock | `framework-probe` transitive | crates.io | HTTP date parsing/formatting |
| `atomic-waker` | `1.1.2` lock | transitive | crates.io | async wake helper |
| `futures-core` | `0.3.32` lock | transitive | crates.io | async trait primitives |
| `futures-channel` | `0.3.32` lock | transitive | crates.io | channel primitives |
| `futures-util` | `0.3.32` lock | transitive | crates.io | async utility adapters |
| `pin-project-lite` | `0.2.17` lock | transitive | crates.io | pin projection macro |
| `smallvec` | `1.15.1` lock | transitive | crates.io | inline small vectors |
| `want` | `0.3.1` lock | transitive | crates.io | demand signaling helper |
| `try-lock` | `0.2.5` lock | transitive | crates.io | lightweight nonblocking lock |
| `itoa` | `1.0.18` lock | transitive | crates.io | integer formatting |
| `libc` | `0.2.185` | `tokio-net-probe` transitive | `../TRUEOS/vendor/libc-0.2.185` | OS ABI bindings |

## Weather Blueprint Boundary

`weather.bp` is the preferred weather/UI proof. It owns the UI2 surface and
`trueos-weather` parsing inside an app VM, builds its own Tokio current-thread
runtime, and keeps the old kernel `ui2-weather-demo` out of the boot task path.

Current transport split:

| Surface | Status | Note |
| --- | --- | --- |
| Tokio runtime/time in blueprint | known-good | `runtime::current_thread_net()` builds and drives refresh cadence |
| Tokio TCP in blueprint | known-good probe | `weather` performs a direct HTTP/TCP proof through the blueprint VNet bridge |
| HTTPS weather body | transitional | uses host fetch CABI until blueprint TLS/Hyper is surfaced |
| UI2 presentation | blueprint-owned | uploads a self-contained RGBA weather panel to the app VM surface |
