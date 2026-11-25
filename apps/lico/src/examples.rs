pub const DEFAULT_PATCH: &str = include_str!("../assets/examples/lead.glicol");

pub const PATCHES: &[(&str, &str)] = &[
    ("Lead and kick", DEFAULT_PATCH),
    (
        "808 drum machine",
        include_str!("../assets/examples/808.glicol"),
    ),
    (
        "Synth drums",
        include_str!("../assets/examples/drums.glicol"),
    ),
    (
        "Acoustic drum kit",
        include_str!("../assets/examples/acoustic_kit.glicol"),
    ),
    (
        "Filter bass",
        include_str!("../assets/examples/bass.glicol"),
    ),
    ("FM bells", include_str!("../assets/examples/fm.glicol")),
    ("Tidepool", include_str!("../assets/examples/tidepool.glicol")),
    (
        "Cosmic soundscape",
        include_str!("../assets/examples/cosmic.glicol"),
    ),
    (
        "Stereo tremolo",
        include_str!("../assets/examples/stereo.glicol"),
    ),
    ("Tape echo", include_str!("../assets/examples/echo.glicol")),
    (
        "Sample pattern",
        include_str!("../assets/examples/pattern.glicol"),
    ),
    (
        "Filter automation",
        include_str!("../assets/examples/automation.glicol"),
    ),
    (
        "Rhai waveshaper",
        include_str!("../assets/examples/waveshaper.glicol"),
    ),
    (
        "MIDI instrument",
        include_str!("../assets/examples/midi.glicol"),
    ),
    (
        "Synthesized piano",
        include_str!("../assets/examples/piano.glicol"),
    ),
    (
        "Piano and 808",
        include_str!("../assets/examples/piano_808.glicol"),
    ),
];
