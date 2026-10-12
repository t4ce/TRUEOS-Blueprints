# Reqwest blocking probe

From the Blueprints repository root, build the probe locally without publishing:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp reqwest_blocking
```

The probe serves four HTTP requests on loopback. It checks a streamed response in two reads, including a server acknowledgement between chunks and a response that outlives its client handle. It then checks a timeout while reading a stalled body, a timeout before response headers, and a successful request after the blocking client's helper runtime has shut down and a new client has been built. Success is logged as `reqwest_blocking: PASS ... requests=4`; any failure logs `reqwest_blocking: FAIL stage=...`.

The same request sequence passed on a Linux host using the actual vendored Reqwest 0.13.3 source in an isolated Cargo harness with a host logging shim and HTTP-only `blocking` feature. The TRUEOS Blueprint builds with the `blocking` and `rustls` features and matching CABI imports. Runtime behavior on TRUEOS and HTTPS/TLS handshakes have not been tested yet.
