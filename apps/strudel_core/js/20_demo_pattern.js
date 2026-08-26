/*
 * Initial pattern expression shown by the HTTP editor.
 *
 * This file deliberately contains an expression rather than an installer IIFE:
 * StrudelVm submits it through the same transactional path used by the UI.
 */
stack(
  sequence(
    instrument("piano", { note: "c4", velocity: 104, pan: -0.18 }),
    [
      instrument("guitar", { note: "g4", velocity: 88, pan: 0.12 }),
      instrument("sax", { note: "bb4", velocity: 82, pan: 0.28 }),
    ],
    instrument("violin", { note: "c5", velocity: 96, pan: 0.08 }),
    [
      instrument("flute", { note: "eb5", velocity: 74, pan: -0.1 }),
      instrument("trumpet", { note: "g4", velocity: 82, pan: -0.24 }),
    ],
  ),
  sequence(
    instrument("drums", { note: 36, velocity: 112 }),
    instrument("conga", { note: 48, velocity: 92 }),
    instrument("bass", { note: "ab1", velocity: 106 }),
    instrument("drums", { note: 38, velocity: 106 }),
  ),
  sequence(
    [instrument("maracas", { note: 78, velocity: 36, pan: 0.55 }), null],
    [null, instrument("banjo", { note: "g5", velocity: 30, pan: -0.5 })],
    [instrument("accordion", { note: "eb6", velocity: 34, pan: 0.4 }), null],
    [null, instrument("voice", { note: "bb5", velocity: 30, pan: -0.35 })],
  ),
)
