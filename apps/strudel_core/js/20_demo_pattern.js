/*
 * Boot-safe upstream Pattern.
 *
 * Keep startup free of nested temporal branches: the HTTP editor installs the
 * full upstream runtime immediately afterward and can commit richer programs.
 * Three one-event sequences still exercise stack, Pattern querying, the TRUEOS
 * instrument vocabulary, native mixing, stereo pan, and HDA playback.
 */
stack(
  sequence(instrument("🎹", { note: "c4", velocity: 92, pan: -0.28 })),
  sequence(instrument("🎚️", { note: "c2", velocity: 104, pan: 0 })),
  sequence(instrument("🪈", { note: "g4", velocity: 62, pan: 0.3 })),
)
