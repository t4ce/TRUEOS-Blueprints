# libc contract probe

This probe calls the actual imported C ABI used by linked Codex dependencies.
It checks character table layout, wide character classification, relative and
absolute filesystem operations, error paths, initialized metadata outputs and
filesystem statistics. Success-shaped stubs cannot satisfy these contracts.

All filesystem changes are under a unique probe-owned directory, which is
removed afterwards. Logs contain fixed case names, numeric errors and timings;
no environment values or file contents are emitted.

Host baseline:

```sh
cargo run --manifest-path probes/libc_contract/Cargo.toml
```

Native build and publish to the user-specified Apps directory:

```sh
sh probes/libc_contract/build_publish.sh
```

Launch on the owned TRUEOS instance with `§§libc_contract`. The package stays in
`probes.json`; publishing it to the Apps directory only makes it launchable.

Native package `1c110129` passed in 33 ms on accepted kernel9, including
asynchronous removal and absence verification of its probe-owned directory.
See [the native evidence](validation/native-kernel9.json).

Filesystem capacity and timestamp mutation currently return honest `ENOSYS`;
the probe checks that unsupported capacity leaves output untouched. Directory
descriptors retain path anchors, and `mkdirat` currently provides sequential
parent/existence semantics without atomic exclusive creation under concurrent
namespace changes. The probe does not claim full POSIX inode semantics.
