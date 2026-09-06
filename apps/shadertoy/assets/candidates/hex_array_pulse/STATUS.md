# Hex Array Pulse v2 — candidate only

`input.glsl` preserves the pasted scene. The retained `kernel.clcpp` adds the
OpenCL no-unroll hints from the host proof; `kernel.bin` and `kernel.spv` are the
corresponding generated artifacts. They are not admitted to the runtime catalog.

The original bake required 8192 bytes of scratch per hardware thread; this
no-unroll trial still requires 2048. The current ShaderToy dispatcher supports
zero scratch. Kernel rejection remains in place. No accepted ABI contract or
runtime package is supplied for this rejected artifact.

The host preview worked at 640×360, including time and mouse input. That does
not establish bare-metal compatibility. See TRUEOS/tools/shadertoy-cpp-offline/
HEX_ARRAY_PULSE.md for the recorded proof and remaining scratch requirement.
