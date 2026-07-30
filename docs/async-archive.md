# Async archive service

Blueprints use the kernel-owned 7z implementation through the `trueos` API:

```rust
let packed = trueos::archive::pack(b"source-dir", b"source.7z").await?;
let unpacked = trueos::archive::unpack(b"source.7z", b"restored-dir").await?;
```

Both calls accept Blueprint-visible filesystem paths. The kernel resolves them
inside the caller's filesystem context before queueing the operation, including
Hull guest calls. The future becomes ready only after all successful filesystem
writes have completed and returns source/archive byte counts plus the file
count.

The service has one bounded FIFO queue (32 requests), at most 64 retained
operation records, and up to three workers. Workers run on distinct AP2+
background executors when available. Profile-identified performance cores are
selected first; efficiency or unknown cores fill the remaining pool positions.
There is no fallback onto the BSP/UI executor.

Current safety limits are 4,096 entries, 16 MiB per source file, 64 MiB total
source or decoded bytes, 64 MiB per archive, a 16 MiB decoder dictionary, and
validated relative paths up to 1,024 bytes and 64 components. The extractor
rejects absolute/traversal paths, duplicate destinations, and file/directory
prefix conflicts before writing.

Unpack is prevalidated but not transactional: a device error can leave files
already written to the destination. Use a fresh staging directory when the
caller needs atomic publication. Dropping the future releases its retained
operation handle; it does not roll back filesystem work that has already begun.
