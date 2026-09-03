# ShaderToy visual Blueprint

Package it with:

```text
!cargo bp shadertoy
```

The app opens a UI4 visual-mode frame at 30 Hz. The kernel brokers admission,
triple buffering, compute completion, and publication, and rejects visual rates
above the current 60 Hz policy ceiling. Use Left/Right or F1-F4 to select the
five reviewed shaders (including Cosmic Strands and Palette Grid); use F1-F5 or
Left/Right to switch; Escape closes the app.

This first ABI is intentionally narrow pending the security analysis. The
Blueprint sends only a catalog id plus pointer-free ShaderToy uniforms. It
cannot send GLSL, SPIR-V, Zebin, GPU addresses, or arbitrary dispatch geometry.
The exact ADL-S SIMD16 artifacts are generated from the reviewed GLSL sources
in the TRUEOS kernel tree with:

```text
make intel-gpu-bake-shadertoy-cpp
```

The original GLSL remains beside the baked kernel sources for review. The
Blueprint embeds the admitted artifact hashes so its catalog and kernel image
can be compared directly.
