# TRUEOS Rust Toolchain Diff

This directory preserves the local rust-src edits needed by the TRUEOS blueprint
SDK after a fresh rustup/toolchain install.

Reapply in one step:

```sh
bash toolchain_diff/reapply_trueos_toolchain.sh nightly-x86_64-unknown-linux-gnu
```

The script resolves the Blueprint repository from its own location, so the
checkout can live anywhere and no machine-specific environment variable is
required.

What it restores:

- TRUEOS cfg hooks in `library/std` for the synthetic `x86_64-unknown-trueos` target.
- The repository's TRUEOS-patched `vendor/libc-0.2.186` copy inside rust-src
  `library/vendor`.
- The rust-src `library/Cargo.lock` pin to `libc 0.2.186`.

Normal Blueprint builds use the repository copy directly as a Cargo source
overlay. The rust-src mirror exists so the toolchain can also be repaired after
a `rustup component remove/add rust-src` cycle.
