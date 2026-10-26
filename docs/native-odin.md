# Native Odin package

From this repository, build and publish the current sibling odin-son compiler:

```sh
cargo bp odin-son
```

`cargo bp odin-son` invokes `../odin-son/trueos/build.sh`, copies the resulting
native Blueprint to `dist/odin-son.bp`, and uses the existing single-app
publication path and SHA-256 filename. Set `TRUEOS_ODIN_SON_DIR` to select a
different source checkout. `ODIN` explicitly selects the installed upstream
compiler and an invalid override fails with an error. Otherwise the builder
looks for `odin` on `PATH`, in `ODIN_SDK_ROOT`/`ODIN_ROOT`, and in common user
installation directories, including `~/Programmme/odin`. It prints the compiler
selected for the build. No Rust wrapper is built.

In the current Shell3, submit `§§odin-son` and Enter to fetch and launch the
published package. An already installed AppDB package also launches by exact
name `odin-son` in CMD mode. The no-args entry opens the usual compiler TUI.

Select `gpu-mandelbrot` with the arrow keys or `j/k`. Press Enter to compile the
byte-embedded Odin source into `mandelbrot.eu.bin`, then `r` to compile that source
again and submit the native SIMD8 body to the existing UI4 compute bridge.
The image supports mouse pan/zoom; Q or Escape in the image returns to the TUI.
The existing CPU boids, GPU boids, hello and discovered source demos remain.

Native submission needs a kernel built with `odin-native-gpgpu` and the
supported Xe-LP device. Successful package publication does not establish GPU
execution; use the runtime compilation and actual GPU completion records.
