# VM carrier core preference

Packages can request a performance core for their VMX carrier through Cargo
metadata:

```toml
[package.metadata.trueos-blueprint]
vm-core-preference = "performance"
```

The default is `"any"`; omitting the key has the same effect. Other values and
non-string values fail packaging. This package setting does not apply to example
targets.

`"performance"` requests a free, eligible P core first. If P cores are absent or
already leased, the kernel may use another eligible carrier. The preference does
not reserve extra cores or change general Tokio worker scheduling.

The packer encodes the request as
`BLUEPRINT_CAP_PREFER_PERFORMANCE_CORE = 1 << 12` (`0x1000`) in the existing TRBP
flags field: a little-endian `u16` at byte offset 6. The low byte still describes
the payload encoding (`3` for TAR+LZ4); the other capability bits retain their
existing meanings. The metadata requires a kernel that recognizes this flag to
affect placement. `apps/ptts` opts in through its ordinary package build.
