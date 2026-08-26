/*
 * Stable boundary between any Strudel-compatible Pattern and the Rust renderer.
 *
 * Output is intentionally an integer matrix. That keeps the no_std Rust parser
 * tiny and avoids leaking Fraction.js/QuickJS object representation across the
 * VM boundary.
 *
 * The adapter also owns the transactional live-expression commit used by the
 * HTTP editor. A failed parse, evaluation, or Pattern check leaves the active
 * pattern and revision untouched.
 */
(function installTrueosAdapter(G) {
  "use strict";

  const core = G.StrudelCore || G.StrudelCoreFallback;
  if (!core) throw new Error("no Strudel core or fallback temporal kernel installed");
  const globalEval = G.eval;

  const runtimeSource = G.StrudelCore ? "upstream" : "fallback";
  const runtimeVersion = G.StrudelCore
    ? String(G.__TRUEOS_UPSTREAM_STRUDEL_VERSION || "unknown")
    : "compat-1";
  const runtimeOrigin = G.StrudelCore
    ? String(G.__TRUEOS_UPSTREAM_STRUDEL_ORIGIN || "embedded")
    : "embedded-fallback";

  let pattern = core.silence;
  let revision = 0;
  let committing = false;
  let cpsNumerator = 1;
  let cpsDenominator = 2;
  let pendingCps = null;
  const heldInputs = new Map();
  const pointerInputs = new Map();

  const INPUT_MIDI = 1;
  const INPUT_KEYBOARD = 2;
  const INPUT_POINTER = 3;
  const KEYBOARD_CHROMATIC = Object.freeze({
    29: 48, 22: 49, 27: 50, 7: 51, 6: 52, 25: 53, 10: 54, 5: 55,
    11: 56, 17: 57, 13: 58, 16: 59, 54: 60, 15: 61, 55: 62, 51: 63, 56: 64,
  });

  // The browser sends one plain JavaScript expression. Install the available
  // pattern-engine exports globally so that expression can use sequence(),
  // stack(), fastcat(), and upstream additions without a namespace wrapper.
  const installedCoreGlobals = [];
  for (const name of Object.keys(core)) {
    if (name === "default" || name === "__esModule") continue;
    G[name] = core[name];
    installedCoreGlobals.push(name);
  }
  installedCoreGlobals.sort();

  const instrumentCatalog = G.__TRUEOS_INSTRUMENT_CATALOG;
  if (!instrumentCatalog) throw new Error("TRUEOS instrument catalog is not installed");

  // Instrument notation is intentionally plain data. It composes with every
  // Strudel constructor: sequence(instrument("piano"), instrument("flute"))
  // and an explicit object field can both reach the native renderer.
  function instrument(name, options) {
    const id = String(name || "").toLowerCase();
    const preset = instrumentCatalog.byId[id];
    if (!preset) throw new RangeError(`unknown TRUEOS instrument: ${name}`);
    const result = {};
    for (const key of Object.keys(preset)) {
      if (key !== "id" && key !== "label" && key !== "icon" && key !== "family" && key !== "snippet") {
        result[key] = preset[key];
      }
    }
    result.instrument = id;
    if (options && typeof options === "object") {
      for (const key of Object.keys(options)) result[key] = options[key];
    }
    return result;
  }
  G.instrument = instrument;
  G.instruments = function instruments() { return instrumentCatalog.entries; };

  function cpsFraction(value) {
    value = finiteNumber(value);
    if (!(value > 0) || !Number.isFinite(value)) throw new RangeError("setcps expects a finite positive number");
    // Keep a stable, bounded rational across the JS/Rust boundary.
    const denominator = 1000000;
    let numerator = Math.round(value * denominator);
    if (numerator <= 0) throw new RangeError("setcps expects a finite positive number");
    let a = numerator;
    let b = denominator;
    while (b) {
      const t = a % b;
      a = b;
      b = t;
    }
    return [numerator / a, denominator / a];
  }

  function setcps(value) {
    if (!committing) throw new Error("setcps is only valid inside a pattern commit");
    pendingCps = cpsFraction(value);
    return core.silence;
  }
  G.setcps = setcps;

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
      const preset = value.instrument && instrumentCatalog.byId[String(value.instrument).toLowerCase()];
      if (preset) {
        waveform = waveformCode(preset.wave);
        velocity = Number(preset.gain) * 127;
        pan = Number(preset.pan) || 0;
      }
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

      if (value.wave !== undefined || value.waveform !== undefined) {
        waveform = waveformCode(value.wave !== undefined ? value.wave : value.waveform);
      }
      if (value.pan !== undefined) pan = Number(value.pan);
    }

    if (note === null) return null;
    return {
      note,
      velocity: clampInteger(velocity, 0, 127),
      waveform,
      panQ15: clampInteger(Math.max(-1, Math.min(1, pan)) * 32767, -32768, 32767),
    };
  }

  function clamp(value, minimum, maximum) {
    return Math.max(minimum, Math.min(maximum, value));
  }

  function inputKey(source, device, control) {
    return `${source}:${device}:${control}`;
  }

  function applyInputs(rows) {
    if (!Array.isArray(rows)) throw new TypeError("applyInputs expects an integer input matrix");
    for (const row of rows) {
      if (!Array.isArray(row) || row.length !== 6) throw new TypeError("invalid performance input row");
      const source = clampInteger(row[0], INPUT_MIDI, INPUT_POINTER);
      const device = clampInteger(row[1], 0, 0xffffffff);
      const control = clampInteger(row[2], 0, 0xffffffff);
      const value = clampInteger(row[3], -0x80000000, 0x7fffffff);
      const gate = Boolean(row[4]);
      const frame = Math.max(0, Math.trunc(Number(row[5])) || 0);

      if (source === INPUT_MIDI) {
        if (control > 127) continue;
        const key = inputKey(source, device, control);
        if (gate && value > 0) {
          heldInputs.set(key, { source, device, control, note: control, velocity: clampInteger(value, 1, 127), frame });
        } else {
          heldInputs.delete(key);
        }
      } else if (source === INPUT_KEYBOARD) {
        const note = KEYBOARD_CHROMATIC[control];
        if (note === undefined) continue;
        const key = inputKey(source, device, control);
        if (gate) {
          heldInputs.set(key, { source, device, control, note, velocity: clampInteger(value || 100, 1, 127), frame });
        } else {
          heldInputs.delete(key);
        }
      } else {
        const key = `${source}:${device}`;
        const pointer = pointerInputs.get(key) || { note: 60, velocity: 96, gate: false, frame };
        if (control === 0) pointer.note = clamp(pointer.note + value / 12, 36, 96);
        else if (control === 1) pointer.velocity = clamp(pointer.velocity - value / 8, 8, 127);
        else continue;
        pointer.gate = gate;
        pointer.frame = frame;
        pointerInputs.set(key, pointer);
      }
    }
    return status();
  }

  function liveRows(blockFrames) {
    const rows = [];
    for (const voice of heldInputs.values()) {
      rows.push([0, blockFrames, 0, blockFrames, Math.round(voice.note), voice.velocity, 0, 0]);
    }
    for (const voice of pointerInputs.values()) {
      if (!voice.gate) continue;
      rows.push([0, blockFrames, 0, blockFrames, Math.round(voice.note), Math.round(voice.velocity), 3, 0]);
    }
    return rows;
  }

  function status() {
    return {
      source: runtimeSource,
      version: runtimeVersion,
      origin: runtimeOrigin,
      revision,
      cpsNumerator,
      cpsDenominator,
      exports: installedCoreGlobals.length,
      heldInputs: heldInputs.size,
      pointerInputs: pointerInputs.size,
    };
  }

  function acceptPattern(nextPattern) {
    if (!nextPattern || typeof nextPattern.queryArc !== "function") {
      throw new TypeError("commitExpression expects a Strudel Pattern");
    }
    pattern = nextPattern;
    if (pendingCps) {
      cpsNumerator = pendingCps[0];
      cpsDenominator = pendingCps[1];
    }
    pendingCps = null;
    revision += 1;
    return status();
  }

  function describeError(error) {
    if (error && error.stack) return String(error.stack);
    if (error && error.message) return String(error.message);
    return String(error);
  }

  function commitExpression(source) {
    if (typeof source !== "string" || source.trim().length === 0) {
      throw new TypeError("commitExpression expects a non-empty JavaScript expression");
    }
    if (committing) {
      throw new Error("nested pattern commits are not allowed");
    }

    committing = true;
    pendingCps = [cpsNumerator, cpsDenominator];
    try {
      let candidate;
      try {
        // Indirect eval executes in global scope, where pattern-engine exports are
        // installed. Parentheses make the contract explicit: one expression whose
        // value is Pattern-like.
        candidate = globalEval(`(
${source}
)`);
      } catch (error) {
        throw new Error(`pattern expression failed: ${describeError(error)}`);
      }

      // Validation occurs before assignment, so failure preserves playback.
      return acceptPattern(candidate);
    } finally {
      pendingCps = null;
      committing = false;
    }
  }

  function queryFrames(
    absoluteStartFrame,
    blockFrames,
    sampleRate,
  ) {
    absoluteStartFrame = Math.trunc(Number(absoluteStartFrame));
    blockFrames = Math.trunc(Number(blockFrames));
    sampleRate = Math.trunc(Number(sampleRate));
    const cps = cpsNumerator / cpsDenominator;
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

    // Live input is additive: temporal patterns retain their independent
    // query clock, while MIDI/keyboard/pointer voices are rendered per block.
    rows.push(...liveRows(blockFrames));

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

  const bridge = Object.freeze({
    core,
    commitExpression,
    applyInputs,
    queryFrames,
    selfTest,
    status,
    source: runtimeSource,
    version: runtimeVersion,
    origin: runtimeOrigin,
    instrumentCatalog: instrumentCatalog.entries,
  });
  Object.defineProperty(G, "__TRUEOS_STRUDEL", {
    value: bridge,
    writable: false,
    configurable: false,
    enumerable: false,
  });
})(globalThis);
