stack(
  sequence(
    instrument("🎹", { note: "c4", velocity: 104, pan: -0.18 }),
    [
      instrument("🎸", { note: "g4", velocity: 88, pan: 0.12 }),
      instrument("🎷", { note: "bb4", velocity: 82, pan: 0.28 }),
    ],
    instrument("🎻", { note: "c5", velocity: 96, pan: 0.08 }),
    [
      instrument("🪈", { note: "eb5", velocity: 74, pan: -0.1 }),
      instrument("🎺", { note: "g4", velocity: 82, pan: -0.24 }),
    ],
  ),
  sequence(
    instrument("🥁", { note: 36, velocity: 112 }),
    instrument("🪘", { note: 48, velocity: 92 }),
    instrument("🎚️", { note: "ab1", velocity: 106 }),
    instrument("🥁", { note: 38, velocity: 106 }),
  ),
  sequence(
    [instrument("🪇", { note: 78, velocity: 36, pan: 0.55 }), null],
    [null, instrument("🪕", { note: "g5", velocity: 30, pan: -0.5 })],
    [instrument("🪗", { note: "eb6", velocity: 34, pan: 0.4 }), null],
    [null, instrument("🎤", { note: "bb5", velocity: 30, pan: -0.35 })],
  ),
)
