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
| `bytes` | `1.11.1` | `../TRUEOS/vendor/bytes-1.11.1` |
| `http` | `1.4.0` | crates.io |
| `http-body` | `1.0.1` | crates.io |
| `httparse` | `1.10.1` | crates.io |
| `httpdate` | `1.0.3` | crates.io |
| `atomic-waker` | `1.1.2` | `../TRUEOS/vendor/atomic-waker-1.1.2` |
| `futures-core` | `0.3.32` | `../TRUEOS/vendor/futures-core-0.3.32` |
| `futures-channel` | `0.3.32` | `../TRUEOS/vendor/futures-channel-0.3.32` |
| `futures-util` | `0.3.32` | crates.io |
| `pin-project-lite` | `0.2.17` | `../TRUEOS/vendor/pin-project-lite-0.2.17` |
| `smallvec` | `1.15.1` | `../TRUEOS/vendor/smallvec-1.15.1` |
| `want` | `0.3.1` | `../TRUEOS/vendor/want-0.3.1` |
| `try-lock` | `0.2.5` | `../TRUEOS/vendor/try-lock-0.2.5` |
| `itoa` | `1.0.18` | `../TRUEOS/vendor/itoa-1.0.18` |
| `libc` | `0.2.185` | `../TRUEOS/vendor/libc-0.2.185` |

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
