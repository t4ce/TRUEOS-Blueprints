# `strudel_core` app

This folder is intended to be copied to `TRUEOS-Blueprints/apps/strudel_core`.

The program keeps one QuickJS `Workbench` alive, installs either the generated upstream bundle or the included fallback, and renders 50 ms PCM blocks while maintaining roughly 300 ms in the existing audio queue.

Important files:

- `src/strudel_vm.rs`: persistent VM installation and integer event queries.
- `src/renderer.rs`: deterministic no_std polyphonic oscillator renderer.
- `src/audio_output.rs`: real `trueos::audio::Stream` queue writes.
- `js/10_trueos_adapter.js`: the stable Pattern/Hap → integer-row ABI.
- `js/20_demo_pattern.js`: composition.
- `js/vendor/strudel-core.bundle.js`: generated upstream bundle or placeholder.

The first target is an audible, repeatable boot-time proof rather than a complete live-coding UI.

A host-auditionable two-second reference render is included at `../../tests/reference-demo-2s.wav`.
