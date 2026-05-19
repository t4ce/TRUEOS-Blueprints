# TRUEOS Rust Toolchain Diff

This directory preserves the local rust-src edits needed by the TRUEOS blueprint
SDK after a fresh rustup/toolchain install.

Reapply in one step:

```sh
TRUEOS_REPO_ROOT=/home/t4ce/REPOS/TRUEOS bash toolchain_diff/reapply_trueos_toolchain.sh nightly-x86_64-unknown-linux-gnu
```

What it restores:

- TRUEOS cfg hooks in `library/std` for the synthetic `x86_64-unknown-trueos` target.
- The TRUEOS vendored `libc-0.2.186` copy inside rust-src `library/vendor`.
- The rust-src `library/Cargo.lock` pin to `libc 0.2.186`.

