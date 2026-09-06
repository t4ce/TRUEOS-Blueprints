Vendored from https://github.com/rayon-rs/rayon/tree/main

Upstream commit: 9254f190aa7ee7521d528e839be3cd0921a04fd8

The upstream license files are preserved in this directory and in
`rayon-core/`.

The public spawn callback and pool-build error use `std::io`, as upstream
does. Preserve `ErrorKind::Unsupported` and raw OS errors; substituting
`core3::io` breaks callers such as Stylo and misclassifies spawn failures.
The default spawner still uses `std::thread`; TRUEOS native worker admission
and shutdown need an explicit adapter. Restoring this API alone does not
enable parallel style traversal.
