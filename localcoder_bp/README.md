# `localcoder_bp`

Minimal TRUEOS portal app scaffold for the future `localcoder` lift.

What it does now:

- exports an unmangled `main(argc, argv)` symbol for the kernel portal loader
- runs as `no_std + alloc`
- uses TRUEOS CABI through `trueos-v`
- demonstrates:
  - argv/env access
  - shell output
  - file read/write
  - network byte fetch
  - shell history access

## Build

From the repo root:

```bash
python3 localcoder_bp/build_bp.py
```

That writes:

```text
localcoder_bp/dist/localcoder_bp.bp
```

The `.bp` format written here matches the current kernel loader expectation:

- magic: `TRBP`
- version: `1`
- flags: `1` for raw, uncompressed payload
- entry hint: `0` so the loader falls back to the `main` symbol lookup

## Run

Place the generated `.bp` into the TRUEOSFS root, then inside TRUEOS:

```text
run
run <id>
run <id> args
run <id> env
run <id> env PWD
run <id> read /path/to/file.txt
run <id> write /tmp/demo.txt hello
run <id> fetch https://example.com
run <id> history
```

## Notes

- This is intentionally a scaffold, not a direct port of the upstream `localcoder`.
- The next step is to extract a host-agnostic core and grow this crate into a TRUEOS frontend.
