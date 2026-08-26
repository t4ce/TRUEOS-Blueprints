/*
 * Stable boundary between any Strudel-compatible Pattern and the Rust renderer.
 *
 * Output is intentionally an integer matrix. That keeps the no_std Rust parser
 * tiny and avoids leaking Fraction.js/QuickJS object representation across the
 * VM boundary.
 */
(function installTrueosAdapter(G) {
  "use strict";

  const core = G.StrudelCore || G.StrudelCoreFallback;
  if (!core) throw new Error("no Strudel core or fallback temporal kernel installed");

  let pattern = core.silence;

  function finiteNumber(value) {
    if (typeof value === "number") return Number.isFinite(value) ? value : NaN;
    if (typeof value === "bigint") return Number(value);
    if (value && typeof value.valueOf === "function") {
      const converted = value.valueOf();
      if (typeof converted === "number" && Number.isFinite(converted)) return converted;
      if (typeof converted === "bigint") return Number(converted);
    }
    if (value && "n" in value && "d" in value) {
      const sign = "s" in value ? Number(value.s) : 1;
      return (sign * Number(value.n)) / Number(value.d);
    }
    const converted = Number(value);
    return Number.isFinite(converted) ? converted : NaN;
  }

  function clampInteger(value, minimum, maximum) {
    value = Math.round(Number(value));
    if (!Number.isFinite(value)) return minimum;
    return Math.max(minimum, Math.min(maximum, value));
  }

  function noteNameToMidi(value) {
    if (typeof value === "number") return clampInteger(value, 0, 127);
    if (typeof value !== "string") return null;
    const trimmed = value.trim();
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) {
      return clampInteger(Number(trimmed), 0, 127);
    }
    const match = /^([a-gA-G])([#b]?)(-?\d+)$/.exec(trimmed);
    if (!match) return null;
    const pitchClass = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 }[
      match[1].toLowerCase()
    ];
    const accidental = match[2] === "#" ? 1 : match[2] === "b" ? -1 : 0;
    const octave = Number(match[3]);
    return clampInteger((octave + 1) * 12 + pitchClass + accidental, 0, 127);
  }

  function waveformCode(value) {
    switch (String(value || "sine").toLowerCase()) {
      case "square":
      case "pulse":
        return 1;
      case "saw":
      case "sawtooth":
        return 2;
      case "tri":
      case "triangle":
        return 3;
      case "noise":
      case "white":
        return 4;
      default:
        return 0;
    }
  }

  function voiceFromValue(value) {
    let note = null;
    let velocity = 96;
    let waveform = 0;
    let pan = 0;

    if (typeof value === "number" || typeof value === "string") {
      note = noteNameToMidi(value);
    } else if (value && typeof value === "object") {
      const rawNote =
        value.midinote !== undefined
          ? value.midinote
          : value.midi !== undefined
            ? value.midi
            : value.note !== undefined
              ? value.note
              : value.n;
      note = noteNameToMidi(rawNote);

      if (value.velocity !== undefined) velocity = value.velocity;
      else if (value.vel !== undefined) velocity = value.vel;
      else if (value.gain !== undefined) velocity = Number(value.gain) * 127;

      waveform = waveformCode(value.wave !== undefined ? value.wave : value.waveform);
      pan = value.pan !== undefined ? Number(value.pan) : 0;
    }

    if (note === null) return null;
    return {
      note,
      velocity: clampInteger(velocity, 0, 127),
      waveform,
      panQ15: clampInteger(Math.max(-1, Math.min(1, pan)) * 32767, -32768, 32767),
    };
  }

  function setPattern(nextPattern) {
    if (!nextPattern || typeof nextPattern.queryArc !== "function") {
      throw new TypeError("setPattern expects a Strudel Pattern");
    }
    pattern = nextPattern;
    return pattern;
  }

  function queryFrames(
    absoluteStartFrame,
    blockFrames,
    sampleRate,
    cpsNumerator,
    cpsDenominator,
  ) {
    absoluteStartFrame = Math.trunc(Number(absoluteStartFrame));
    blockFrames = Math.trunc(Number(blockFrames));
    sampleRate = Math.trunc(Number(sampleRate));
    const cps = Number(cpsNumerator) / Number(cpsDenominator);
    if (
      absoluteStartFrame < 0 ||
      blockFrames <= 0 ||
      sampleRate <= 0 ||
      !(cps > 0) ||
      !Number.isFinite(cps)
    ) {
      throw new RangeError("invalid TRUEOS query geometry");
    }

    const absoluteEndFrame = absoluteStartFrame + blockFrames;
    const cycleBegin = (absoluteStartFrame / sampleRate) * cps;
    const cycleEnd = (absoluteEndFrame / sampleRate) * cps;
    const haps = pattern.queryArc(cycleBegin, cycleEnd);
    const rows = [];

    for (const hap of haps) {
      const whole = hap.whole || hap.part;
      if (!whole) continue;
      const wholeBegin = finiteNumber(whole.begin);
      const wholeEnd = finiteNumber(whole.end);
      if (!Number.isFinite(wholeBegin) || !Number.isFinite(wholeEnd) || wholeEnd <= wholeBegin) {
        continue;
      }

      const voice = voiceFromValue(hap.value);
      if (!voice || voice.velocity === 0) continue;

      const onsetFrame = Math.round((wholeBegin / cps) * sampleRate);
      const releaseFrame = Math.round((wholeEnd / cps) * sampleRate);
      const clippedStart = Math.max(absoluteStartFrame, onsetFrame);
      const clippedEnd = Math.min(absoluteEndFrame, releaseFrame);
      if (clippedEnd <= clippedStart) continue;

      rows.push([
        clippedStart - absoluteStartFrame,
        clippedEnd - absoluteStartFrame,
        Math.max(0, clippedStart - onsetFrame),
        Math.max(1, releaseFrame - onsetFrame),
        voice.note,
        voice.velocity,
        voice.waveform,
        voice.panQ15,
      ]);
    }

    rows.sort((a, b) => a[0] - b[0] || a[4] - b[4] || a[1] - b[1]);
    return rows;
  }

  function selfTest() {
    const probe = core.sequence("a", ["b", "c"]);
    return probe
      .queryArc(0, 1)
      .map((hap) => {
        const whole = hap.whole || hap.part;
        return `${hap.value}@${finiteNumber(whole.begin).toFixed(6)}-${finiteNumber(
          whole.end,
        ).toFixed(6)}`;
      })
      .join("|");
  }

  G.__TRUEOS_STRUDEL = Object.freeze({
    core,
    setPattern,
    queryFrames,
    selfTest,
    source: G.StrudelCore ? "upstream" : "fallback",
  });
})(globalThis);
