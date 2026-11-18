# Lily VFX capture

`lilyvfx` renders the twelve Spirit backgrounds once on TRUEOS and saves the
native GPU pixels to TRUEOSFS. It uses one 256×256 surface per effect with the
same control words as the offline reference. A compatible TRUEOS kernel with
the `lilyvfx` capture service is required.

Build the on-demand Blueprint locally from the TRUEOS-Blueprints checkout:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp lilyvfx
```

In the TRUEOS checkout, generate the reference and its control pages:

```sh
make -C tools/lilly/spirit-vfx-offline capture-background PREFIX=bld/spirit-vfx-preview
```

Place `dist/lilyvfx.bp` at `/apps/common/lilyvfx/lilyvfx.bp` on TRUEOSFS and
`bld/spirit-vfx-preview.controls` at `/apps/common/lilyvfx/preview.controls`.
An existing Blueprint can start the capture through the shared path:

```rust
trueos::vshell::launch_with_destination(
    "common/lilyvfx/lilyvfx.bp",
    "",
    trueos::vshell::LaunchDestination::Headless,
)?;
```

For an AppDB launch, write `common/lilyvfx/lilyvfx.bp` to `vFile:appdb-install`
and choose `lilyvfx` in Shell3. With no arguments, the capture writes
`/apps/common/lilyvfx/native.bgra`. An argv launch can pass two filesystem
paths to use a different input and output.

The output contains the twelve full cells in shader ID order 2–13, each
packed as top-to-bottom premultiplied BGRA8 (3,145,728 bytes total). Copy the
file back to the host and run this comparison from the TRUEOS checkout:

```sh
python3 tools/lilly/spirit-vfx-offline/compare_backgrounds.py \
  bld/spirit-vfx-preview.bgra native.bgra --output bld/spirit-vfx-comparison.png
```
