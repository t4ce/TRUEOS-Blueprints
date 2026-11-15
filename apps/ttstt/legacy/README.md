# Migration reference only

These former kernel files are not compiled or registered by ttstt.
`../src/ttstt_service.rs` now runs TTS using native std filesystem access,
cooperative slices and the existing Blueprint audio ABI. The archived
controller includes dormant STT, capture diagnostics and kernel-only HWP
policy; retaining it does not enable those facilities.
