# ShaderToy compiler settings that helped

The important win was passing **`-cl-fast-relaxed-math` to ocloc/IGC**:

```text
ocloc compile -spirv_input ... -options "-cl-fast-relaxed-math"
```

The host OpenCL preview must pass the same option to `clBuildProgram`.
Adding it only to Clang did not produce the same improvement in our SPIR-V path.
Backend options are recorded in the authenticated bake manifest and pinned
profile: the same SPIR-V can produce materially different executables.

Protean, Nguyen, Palette Grid and Cosmic Strands use the separate
`adls-4680-r0c-shadertoy.json` profile. Mandelbrot and the cube field retain the
strict profile; the cube's sine-based hash amplifies tiny numeric differences
into changed geometry.

The flag permits finite-math assumptions, unsafe arithmetic transformations,
MAD contraction and relaxed signed-zero, denormal and library precision rules.
This is a visual-effect choice, not a default for numerical kernels. Large-time
Protean inputs can drift from the strict-math pattern.

Explicit `native_sin/cos/exp/divide/recip/rsqrt` trials gave no further measurable
speedup or image changes versus that backend option on the pinned compiler.
Keep SIMD16 and zero scratch/SLM admission checks. Neither those checks nor
artifact authentication was weakened for the speedup.

At 1440p on the local UHD 770/OpenCL path, the compiler and row-dispatch update
reduced Protean from about 972 to 217 ms. This is not a measurement of the
bare-metal machine's clocks or frame rate. Detailed evidence is in TRUEOS's
`tools/shadertoy-cpp-offline/RUNTIME_PERFORMANCE.md`.

The imported native gallery, audio and ParticleCraft programs retain their
existing bake profiles and explicit `native_*` math. Their import is byte-for-byte;
the ShaderToy relaxed-math profile is not retroactively applied to them. Each
`assets/<program>/kernel.manifest.json` records the actual compiler options.
