# Intel Gen12 shader stage state for bfly

The package [manifest](manifest.json) identifies the stage byte ranges and hashes. The [R8 cache contract](../intel-r8-contract.md) gives the allocations, image views, mip generation, sampler, and barriers. The tables below give the hardware state that connects those resources to the shader ISA.

Mesa ANV 26.0.8 compiled the same shader words for Raptor Lake-S `8086:a780` revision 04 and Alder Lake-S `8086:4680` revision 0C. Both offline builds used the matching Intel no-op DRM shim, with the PCI device and revision checked through libdrm. Their ISA also matched the physical A780 capture, including the compiler's display VS padding described in the manifest. The coupled capture drew and read back both passes on physical A780. Recording the same Vulkan draw sequence through the 4680 shim produced identical fixed-function packet fields for the bake and display draws after excluding GPU address relocations. Physical execution of the packet sequence is verified on A780.

Each row lists a command's DWORDs in order. `KSP` fields are offsets from instruction state base. Use the package stage bodies in manifest order, with each kernel start aligned to 64 bytes. `UBO` fields are GPU addresses of the 160-byte uniform block. Binding table, sampler, target, and viewport pointers come from the native draw's mapped state and resources.

## R8 mask bake

The vertex buffer holds five four-point patches, each vertex stored as `f32 x, f32 y, f32 straight_edge` with stride 12. Each layer uses a single sequential draw of 20 vertices and one instance. The shader divides a cubic into up to 16 isolines so that the hull shader can request 1,024 segments while the tessellation engine stays within 64 subdivisions per isoline.

| Command | Captured DWORDs |
| --- | --- |
| `URB_ALLOC_VS` | `78580001 00801000 0a800a80` |
| `URB_ALLOC_HS` | `78590001 03206401 04800480` |
| `URB_ALLOC_DS` | `785a0001 0560ac00 0a800a80` |
| `URB_ALLOC_GS` | `785b0001 00801000 00000000` |
| `VERTEX_ELEMENTS` | `78090005 02850000 11230000 02d80008 12230000 7e870000 22220000` |
| `VF_INSTANCING` | `78490001 00000000 00000001`, `78490001 00000001 00000001`, `78490001 00000002 00000000` |
| `VF_SGVS` | `784a0000 e0024002` |
| `VF_SGVS_2` | `78560001 b0020002 00000003` |
| `VF_COMPONENT_PACKING` | `78550003 00000a13 00000000 00000000 00000000` |
| `VF_TOPOLOGY` | `784b0000 00000023` |
| `VS` | `78100007 <VS8 KSP lo/hi> 00000000 00000000 00000000 00200800 88400405 00000000` |
| `HS` at draw | `781b0007 00080000 a0014f03 <HS8 KSP lo/hi> 00000000 00000000 01340000` |
| `TE` | `781c0003 00800121 427c0000 42800000 00000000` |
| `DS` | `781d0009 <DS8 KSP lo/hi> 00080000 00000000 00000000 00507800 44200609 00000000 00000000 00000000` |
| `SBE` | `781f0004 30200000 00000000 00000000 ffffffff ffffffff` |
| `PS` | `7820000a <PS8 KSP lo/hi> 40040000 00000000 00000000 1f800003 00020002 <PS8 KSP lo/hi> <PS16 KSP lo/hi>` |
| `PS_EXTRA` | `784f0000 80000000` |
| `CLIP` | `78120002 00040400 c4000001 0003ffe0` |
| `SF` | `78130002 00080402 00000000 02004808` |
| `RASTER` | `78500003 04210003 00000000 00000000 00000000` |
| `WM` | `78140000 80000040` |
| `PS_BLEND` | `784d0000 41986600` |
| `3DPRIMITIVE` | `7b000808 00000000 00000014 00000000 00000001 00000000 00000000 00000000 00000000 00000000` |

The VS SIMD8 input includes the two memory attributes and the synthetic element 2. SGVS injects instance ID into W and starting instance into Y. Bind the raw UBO surface at index 1 of the HS and DS tables with the reserved descriptor at index 0. The VS uses zero entries. The bake PS has one binding-table entry for the R8 layer render target at index 0.

`WingView` stores the projection at byte 0, local bounds at 64, framebuffer dimensions at 80, and wing orientation at 96. HS loads five 32-byte units from UBO offset 0 in constant-buffer slot 3 (`CONSTANT_HS` length word `00050000`). DS loads two units from offset 0 in slot 2 and two from offset 96 in slot 3 (`CONSTANT_DS` length word `00020002`). The capture disables unused VS and PS constant slots with `CONSTANT_ALL` header `786d1100`. It allocates VS, HS, DS, and PS constant URB with payloads `00000008`, `00080008`, `00100008`, `00180008` respectively, and leaves GS disabled.

## Sampled strips and hinge animation

Draw four sequential vertices and 24 instances from zero. Instances 0–7 draw the panel backgrounds. Instances 8–23 each draw one wing using two triangles, with vertex and instance IDs supplied through SGVS. The synthetic vertex element uses buffer index 31 and stores zeros in all four components before SGVS overrides them. The draw needs no memory vertex or index buffer. Disable tessellation and geometry state between the two passes.

| Command | Captured DWORDs |
| --- | --- |
| `URB_ALLOC_VS` | `78580001 00801001 0df80df8` |
| `URB_ALLOC_HS` | `78590001 00801000 00000000` |
| `URB_ALLOC_DS` | `785a0001 00801000 00000000` |
| `URB_ALLOC_GS` | `785b0001 00801000 00000000` |
| `VERTEX_ELEMENTS` | `78090001 7e870000 22220000` |
| `VF_INSTANCING` | `78490001 00000000 00000000` |
| `VF_SGVS` | `784a0000 e000c000` |
| `VF_SGVS_2` | `78560001 b0008000 00000001` |
| `VF_COMPONENT_PACKING` | `78550003 0000000f 00000000 00000000 00000000` |
| `VF_TOPOLOGY` | `784b0000 00000005` |
| `VS` | `78100007 <VS8 KSP lo/hi> 00080000 00000000 00000000 00200800 88400405 00000000` |
| `SBE` | `781f0004 31201020 00000000 0000000e ffffffff ffffffff` |
| `SBE_SWIZ` | `78510009 00010000 00030002 00000000 00000000 00000000 00000000 00000000 00000000 00000000 00000000` |
| `PS` | `7820000a <PS8 KSP lo/hi> 480c0000 00000000 00000000 1f800007 0006130a <PS32 KSP lo/hi> <PS16 KSP lo/hi>` |
| `PS_EXTRA` | `784f0000 91800100` |
| `CLIP` | `78120002 00040400 d4000001 0003ffe0` |
| `SF` | `78130002 00080402 00000000 02004808` |
| `RASTER` | `78500003 04a11003 00000000 00000000 00000000` |
| `WM` | `78140000 80000840` |
| `PS_BLEND` | `784d0000 61986600` |
| `3DPRIMITIVE` | `7b000808 00000000 00000004 00000000 00000018 00000000 00000000 00000000 00000000 00000000` |

Use separate stage binding tables. The VS table holds the reserved descriptor at index 0 and the raw UBO surface at index 1. The PS table holds the RGBA8 draw target at index 0, the reserved descriptor at 1, and the R8 array sample view at 2. The PS samples at sampler index 0. SBE routes four varyings in order, with locations 1–3 flat. WM supplies perspective pixel UV interpolation. The fragment shader also consumes pixel position and can discard a fragment outside its panel. Use the premultiplied display blend from the cache contract.

`WingDisplay` stores two column-major matrices at bytes 0 and 64, side bounds at 128, and four floats at 144 for width, height, time in seconds, and nearest camera distance. The preview uses unit scale, camera distance 6 and focal length 8,000. VS loads all five 32-byte units from UBO offset 0 in constant-buffer slot 3 (`CONSTANT_VS` length word `00050000`). The capture clears the PS constants with `CONSTANT_ALL` header `786d1000` and allocates VS and PS constant URB with payloads `00000010` and `00100010`, respectively. Only time changes on a regular presentation tick. Refresh the framebuffer dimensions and projection on resize. Match the viewport Y direction to the shader's pixel-space coordinates as described in the cache contract.

## TRUEOS integration

Keep the stage bytes in the Blueprint and use the kernel's authenticated `app_shaders::words` accessor. Render0 can reuse the existing resident-resource lifetime, UI4 direct target mapping and release fence in `src/intel/render/primary.rs` and `coverage_mask.rs`. The encoder needs a typed four-vertex instanced draw, an R8 array sample view and independent VS and PS resource tables. The existing sampled-triangle draw uses authored position and UV input, a 2D RGBA8 texture descriptor and one instance. The existing R8 coverage draw uses font contracts. The bake needs a separate four-control-point tessellation contract and the cache needs mip generation and a write-to-sample transition. These stage packets belong inside the renderer's validated batch, alongside its native state base addresses, synchronization and completion bookkeeping.
