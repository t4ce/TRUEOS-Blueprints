# Native download diagnostic

`download-smoke` compares the existing kernel HTTPS paths through production
Blueprint APIs. It has no HTTP client or TLS implementation of its own.

From `TRUEOS-Blueprints`, build and refresh the app normally:

```sh
cargo bp download-smoke
```

Launch `§§download-smoke` through the usual online AppDB workflow. The default
matrix makes eight requests, once each, with a one-second interval. General
byte downloads run first, starting with the direct www PNG control. The four
URLs are the www Wikipedia globe PNG, its apex-hostname counterpart, the www
portal page, and the portal's pinned `index-8dfbdb3a7a.js` script. The script
URL represents the observed portal version and may change independently.

Both routes use the real kernel network path:

- `general`: `netfs::fetch_bytes`, the standard general-byte request profile.
- `image`: `ui4_scene::hull_image_request`, the same image service as Hull.
- `general-happy`: optional `netfs::fetch_bytes_happy_eyeballs`, the top-level
  Hull navigation policy, racing the initial IPv6 and IPv4 connection.

Optional arguments select a smaller diagnostic:

```text
download-smoke --route general --url https://www.wikipedia.org/portal/wikipedia.org/assets/img/Wikipedia-logo-v2.png
download-smoke --route image --url https://www.wikipedia.org/portal/wikipedia.org/assets/img/Wikipedia-logo-v2.png
download-smoke --route general-happy --url https://wikipedia.org/
download-smoke --list
```

Repeat `--url` up to eight times to replace the default URL set. `--route both`
uses general first, then image. `--timeout-ms` bounds each operation at 20 seconds
by default, with an allowed range of 1–60 seconds. There are no warmups,
repetitions, or automatic retries after a completed HTTP response. Kernel
redirect/recovery behavior remains the behavior of the selected production API.

Records begin with `download-smoke-app: ` and use the
`download_smoke_app_v1` JSON schema. Each case has begin and request records
with its run, case index, route, URL without query/fragment, operation or ticket,
API error code, and elapsed cycles. General-byte successes additionally report
length, SHA-256, the first 16 bytes in hex, and PNG signature admission. Cycle
timings use LFENCE/RDTSC; the microsecond values have the system clock's 1 ms
resolution.

API codes are not HTTP statuses. An image request of a page or script can
download successfully and then fail image decoding. Correlate the case with
kernel per-hop HTTP diagnostics before classifying it as a download failure.
An image READY result proves fetch, decode, upload preparation, and retained
sprite installation; this diagnostic does not draw or verify GPU pixels.

The summary reports API successes separately from completed cases. Exact
duplicates can appear when terminal output and semantic logs are captured
together; deduplicate identical JSON records by run and case.

On a development host, `cargo run --manifest-path apps/download-smoke/Cargo.toml
-- --list` prints the finite matrix without sending network requests. Actual
downloads require TRUEOS.
