# Aiekick displaced reflective sphere — candidate only

`input.glsl` is the normalized user-pasted source, retaining its author/license
notice. It requires a cubemap on iChannel0 and a mipmapped 2D image on iChannel1
(explicit LOD 0–4). The original assets and sampler settings are not available.

The current ShaderToy compute ABI has no channel bindings or textureLod bridge.
GLSL syntax validation passed with inferred sampler declarations, but no native
C++ artifact was generated. This shader cannot yet be added as an executable
runtime entry. See TRUEOS/tools/shadertoy-cpp-offline/CANDIDATE_CHECKS.md.
