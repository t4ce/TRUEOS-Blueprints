# Local changes

Based on egui-wgpu 0.36.2 from crates.io.

Use `TextureFormat::has_srgb_suffix()` to match the local wgpu 30 fork, which includes this API rename ahead of the published release. Rendering behavior is unchanged.
