/*
 * Replace only this file to compose a different app.
 * No mini notation, editor, browser, transpiler or WebAudio is involved.
 */
(function installDemoPattern(G) {
  "use strict";
  const S = G.StrudelCore || G.StrudelCoreFallback;
  const sequence = S.sequence;
  const stack = S.stack;

  const lead = sequence(
    { note: "c4", velocity: 104, wave: "triangle", pan: -0.18 },
    [
      { note: "g4", velocity: 88, wave: "sine", pan: 0.12 },
      { note: "bb4", velocity: 82, wave: "sine", pan: 0.28 },
    ],
    { note: "c5", velocity: 96, wave: "triangle", pan: 0.08 },
    [
      { note: "eb5", velocity: 74, wave: "sine", pan: -0.1 },
      { note: "g4", velocity: 82, wave: "triangle", pan: -0.24 },
    ],
  );

  const bass = sequence(
    { note: "c2", velocity: 112, wave: "square", pan: 0 },
    { note: "c2", velocity: 92, wave: "square", pan: 0 },
    { note: "ab1", velocity: 106, wave: "square", pan: 0 },
    { note: "bb1", velocity: 106, wave: "square", pan: 0 },
  );

  const sparks = sequence(
    [{ note: "c6", velocity: 36, wave: "sine", pan: 0.55 }, null],
    [null, { note: "g5", velocity: 30, wave: "sine", pan: -0.5 }],
    [{ note: "eb6", velocity: 34, wave: "sine", pan: 0.4 }, null],
    [null, { note: "bb5", velocity: 30, wave: "sine", pan: -0.35 }],
  );

  G.__TRUEOS_STRUDEL.setPattern(stack(lead, bass, sparks));
})(globalThis);
