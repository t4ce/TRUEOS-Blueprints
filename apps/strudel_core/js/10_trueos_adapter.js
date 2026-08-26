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
  let releaseLookbackFrames = 960;
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
    result.instrument = preset.id;
    if (options && typeof options === "object") {
      for (const key of Object.keys(options)) result[key] = options[key];
    }
    return result;
  }
  G.instrument = instrument;
  G.instruments = function instruments() { return instrumentCatalog.entries; };

  // Narrow sound/control compatibility layer.  The vendored upstream slice is
  // deliberately temporal-only, so these familiar Strudel spellings describe
  // data for TRUEOS's native renderer instead of constructing a WebAudio graph.
  // Values which are Patterns are deliberately kept as controls until query
  // time; this is what makes `lpf(sine.range(...))` and friends live.
  const PatternProto = core.Pattern && core.Pattern.prototype;
  if (!PatternProto) throw new Error("Strudel core has no Pattern prototype");
  const isPattern = (value) => Boolean(value && typeof value.queryArc === "function");
  // Keep the temporal vendor deliberately small, but do not silently discard
  // temporal operators supplied by a fuller upstream build.
  const upstreamMask = typeof PatternProto.mask === "function" ? PatternProto.mask : null;
  const mapValue = (source, fn) => source.withValue(fn);
  const copyValue = (value) => {
    if (value && typeof value === "object" && !Array.isArray(value) && !isPattern(value)) {
      const copy = {};
      for (const key of Object.keys(value)) copy[key] = value[key];
      return copy;
    }
    return { note: value };
  };
  const control = (source, key, value) => {
    // Mini strings in control positions are signals, not literal metadata.
    if (typeof value === "string" && ["gain", "lpf", "lpq", "room", "shape", "delay", "postgain", "bpf"].includes(key)) {
      value = miniCompat(value);
    }
    return mapValue(source, (event) => {
    const next = copyValue(event);
    next[key] = value;
    return next;
    });
  };
  const controlMethods = {
    clip: "clip", attack: "attack", decay: "decay", sustain: "sustain", release: "release", lpf: "lpf", lpq: "lpq", lpenv: "lpenv",
    lpd: "lpd", lpa: "lpa", ftype: "ftype", room: "room", shape: "shape",
    postgain: "postgain", delay: "delay", bpf: "bpf", gain: "gain", bank: "bank",
    s: "s",
  };
  for (const [method, key] of Object.entries(controlMethods)) {
    PatternProto[method] = function trueosControl(value) { return control(this, key, value); };
  }
  // `add(note(12))` is a pitch transposition in the native representation.
  // Upstream's arithmetic `add` is a non-configurable getter and already
  // understands `note(12)`. The emergency fallback has no such method.
  if (typeof PatternProto.add !== "function") {
    PatternProto.add = function trueosAdd(amount) {
      return mapValue(this, (event) => {
        const next = copyValue(event);
        next.add = amount;
        return next;
      });
    };
  }
  function temporalPattern(source, select) {
    // withHaps is the public upstream constructor hook. Constructing Pattern
    // directly works in the fallback but bypasses upstream's query State.
    if (typeof source.withHaps === "function") return source.withHaps((haps) => haps.filter(select));
    return new core.Pattern((begin, end) => source.queryArc(begin, end).filter(select));
  }
  function mapHaps(source, map) {
    if (typeof source.withHaps === "function") return source.withHaps(map);
    return new core.Pattern((begin, end) => map(source.queryArc(begin, end)));
  }
  function deterministicUnit(hap) {
    // A stable integer hash, deliberately independent of query ordering and
    // global RNG. Revision makes a newly committed pattern a new variation,
    // while a lookahead/block split cannot change an existing event's choice.
    const whole = hap.whole || hap.part;
    let hash = (revision ^ 0x9e3779b9) >>> 0;
    const begin = Math.round(finiteNumber(whole && whole.begin) * 1048576) >>> 0;
    const end = Math.round(finiteNumber(whole && whole.end) * 1048576) >>> 0;
    hash = Math.imul(hash ^ begin, 0x85ebca6b) >>> 0;
    hash = Math.imul(hash ^ end, 0xc2b2ae35) >>> 0;
    hash ^= hash >>> 16;
    return (hash >>> 0) / 4294967296;
  }
  PatternProto.rarely = function trueosRarely(transform) {
    if (typeof transform !== "function") throw new TypeError("rarely expects a function");
    const source = this;
    return mapHaps(source, (haps) => haps.map((hap) => {
      // Strudel's `rarely` is a low-probability variation. One in eight is
      // intentionally conservative and avoids an audible global RNG stream.
      if (deterministicUnit(hap) >= 0.125) return hap;
      // Restrict this native compatibility implementation to transforms that
      // preserve one event's timing (add(note(12)) is the important case).
      try {
        const changed = transform(core.sequence(hap.value));
        if (!isPattern(changed)) throw new TypeError("rarely transform must return a Strudel Pattern");
        const replacement = changed.queryArc(0, 1)[0];
      return replacement ? hap.withValue(() => replacement.value) : hap;
      } catch (_) {
        // A native control Pattern can lack arithmetic methods from a complete
        // WebAudio build; leave this event intact instead of rejecting audio.
        return hap;
      }
    }));
  };
  PatternProto.superimpose = function trueosSuperimpose(transform) {
    if (typeof transform !== "function") throw new TypeError("superimpose expects a function");
    return core.stack(this, transform(this));
  };
  function numericMaskMini(source) {
    const text = String(source).trim().replace(/[<>\[\]()]/g, " ");
    const values = [];
    for (const match of text.matchAll(/(?:^|\s)(-?(?:\d+\.?\d*|\.\d+))(?:@(\d+))?(?=\s|$)/g)) {
      const value = Number(match[1]);
      const count = Math.max(1, Number(match[2] || 1));
      for (let index = 0; index < count; index += 1) values.push(value);
    }
    return values.length ? core.sequence(...values) : null;
  }
  function truthAt(controlPattern, cycle, repeatEachCycle) {
    // The renderer asks a little before frame zero for release tails. The
    // upstream Fraction query rejects some negative decimal inputs; numeric
    // mini masks are cycle-periodic, so normalize those safely.
    if (repeatEachCycle) cycle -= Math.floor(cycle);
    const epsilon = 1 / 65536;
    const haps = controlPattern.queryArc(cycle, cycle + epsilon);
    return haps.length && finiteNumber(haps[0].value) !== 0;
  }
  PatternProto.mask = function trueosMask(mask) {
    // `<0@4 1@16>` is common live-code shorthand: `@N` means N successive
    // mask steps here. The minimal vendor mini parses it as a duration, so
    // normalize numeric mask strings before handing temporal work to Pattern.
    const numericMini = typeof mask === "string" ? numericMaskMini(mask) : null;
    const controlPattern = isPattern(mask) ? mask : numericMini;
    if (controlPattern) {
      return temporalPattern(this, (hap) => {
        const whole = hap.whole || hap.part;
        return Boolean(whole && truthAt(controlPattern, finiteNumber(whole.begin), Boolean(numericMini)));
      });
    }
    if (upstreamMask) return upstreamMask.call(this, mask);
    throw new TypeError("mask expects a numeric mini string or Pattern-like control");
  };

  function signal(fn) {
    // Do not subclass upstream Pattern here: its numeric operators are bound
    // to upstream's control-pattern implementation, which this audio-free
    // slice intentionally does not ship.  A tiny queryArc object is enough for
    // native control resolution and retains the familiar signal spelling.
    const out = {
      queryArc(begin, end) { return [{ whole: { begin, end }, part: { begin, end }, value: fn(begin) }]; },
    };
    out.range = (low, high) => signal((time) => low + ((fn(time) + 1) * 0.5) * (high - low));
    out.mul = (other) => signal((time) => fn(time) * scalarAt(other, time));
    out.add = (other) => signal((time) => fn(time) + scalarAt(other, time));
    out.fast = (factor) => signal((time) => fn(time * Number(factor)));
    out.slow = (factor) => signal((time) => fn(time / Number(factor)));
    return out;
  }
  function scalarAt(value, time) {
    if (isPattern(value)) {
      const haps = value.queryArc(time, time + 1 / 65536);
      return haps.length ? finiteNumber(haps[0].value) : 0;
    }
    return finiteNumber(value);
  }
  G.sine = signal((time) => Math.sin(time * Math.PI * 2));
  G.cosine = signal((time) => Math.cos(time * Math.PI * 2));
  G.saw = signal((time) => 2 * (time - Math.floor(time + 0.5)));
  G.perlin = signal((time) => Math.sin(time * 12.9898 + Math.sin(time * 4.1414)));

  function miniCompat(source) {
    try { return core.mini(String(source)); } catch (_) {
      // The temporal vendor intentionally omits mini's Euclidean dependency.
      // Preserve useful notes/sounds and their rate suffixes for native use.
      const text = String(source);
      const hasPitchOrSound = /[a-zA-Z]/.test(text);
      const token = hasPitchOrSound
        ? /[a-gA-G][#b]?\-?\d+|[a-zA-Z_]+/g
        : /[-+]?\d*\.?\d+(?:@\d+)?/g;
      const items = [];
      for (const raw of text.match(token) || []) {
        const repeated = /^(.+?)@(\d+)$/.exec(raw);
        const value = repeated ? repeated[1] : raw;
        const copies = repeated ? Math.max(1, Number(repeated[2])) : 1;
        for (let index = 0; index < copies; index += 1) items.push(hasPitchOrSound ? value : Number(value));
      }
      const multiplier = /\*(\d+)/.exec(text);
      if (multiplier) {
        const original = items.slice();
        for (let copy = 1; copy < Number(multiplier[1]); copy += 1) items.push(...original);
      }
      return core.sequence(...(items.length ? items : [null]));
    }
  }
  G.note = function note(value) {
    const source = isPattern(value) ? value : typeof value === "string" ? miniCompat(value) : core.sequence(value);
    return mapValue(source, (event) => ({ note: event }));
  };
  G.s = function sound(value) {
    const source = isPattern(value) ? value : typeof value === "string" ? miniCompat(value) : core.sequence(value);
    return mapValue(source, (event) => ({ s: event }));
  };
  G.add = function add(value) { return (source) => source.add(value); };

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

  function filterTypeCode(value) {
    if (typeof value === "number") return clampInteger(value, 0, 2);
    switch (String(value || "12db").toLowerCase()) {
      case "ladder": return 1;
      case "24db": return 2;
      case "12db":
      default: return 0;
    }
  }

  function sampleSourceId(bank, sound) {
    let hash = 0xcbf29ce484222325n;
    for (const byte of `${bank}:${sound}`) {
      hash ^= BigInt(byte.charCodeAt(0));
      hash = BigInt.asUintN(64, hash * 0x100000001b3n);
    }
    return Number(hash & 0x1fffffffffffffn);
  }

  function valueAt(value, cycle) {
    if (!isPattern(value)) return value;
    const haps = value.queryArc(cycle, cycle + 1 / 65536);
    return haps.length ? valueAt(haps[0].value, cycle) : 0;
  }

  function drumPreset(sound) {
    const id = String(sound || "").toLowerCase().split(":")[0];
    if (id === "bd" || id === "kick") return { note: 36, wave: "sine", lpf: 900, shape: 0.32 };
    if (id === "hh" || id === "oh" || id === "ch") return { note: 78, wave: "noise", lpf: 8500, shape: 0.45 };
    if (id === "sd" || id === "rim" || id === "rd") return { note: 38, wave: "noise", lpf: 4200, shape: 0.22 };
    return null;
  }

  function voiceFromValue(value, cycle, sampleRate) {
    let note = null;
    let velocity = 96;
    let waveform = 0;
    let pan = 0;
    let sourceId = 1;
    let lpf = 0;
    let lpq = 0;
    let room = 0;
    let delay = 0;
    let phaser = 0;
    let shape = 0;
    let fm = 0;
    let fmRate = 1;
    let kind = 1;
    let sampleBegin = 0;
    let sampleEnd = 0;
    let flags = 0;
    let attack = 0.005;
    let decay = 0;
    let sustain = 1;
    let release = 0.02;
    let filterAttack = 0;
    let filterDecay = 0;
    let filterEnvOctaves = 0;
    let filterType = 0;
    let clip = 1;

    if (typeof value === "number" || typeof value === "string") {
      note = noteNameToMidi(value);
    } else if (value && typeof value === "object") {
      const preset = value.instrument && instrumentCatalog.byId[String(value.instrument).toLowerCase()];
      if (preset) {
        sourceId = instrumentCatalog.entries.indexOf(preset) + 1;
        waveform = waveformCode(preset.wave);
        velocity = Number(preset.gain) * 127;
        pan = Number(preset.pan) || 0;
        lpf = Number(preset.lpf) || 0;
        lpq = Number(preset.lpq) || 0;
        room = Number(preset.room) || 0;
        delay = Number(preset.delay) || 0;
        phaser = Number(preset.phaser) || 0;
        shape = Number(preset.shape) || 0;
        fm = Number(preset.fm) || 0;
        fmRate = Number(preset.fmRate) || 1;
      }
      // Resolve signal/control Patterns only at the queried cycle. This avoids
      // carrying JS objects over the integer VM boundary.
      const rawNote =
        value.midinote !== undefined
          ? valueAt(value.midinote, cycle)
          : value.midi !== undefined
            ? valueAt(value.midi, cycle)
            : value.note !== undefined
              ? valueAt(value.note, cycle)
              : valueAt(value.n, cycle);
      note = noteNameToMidi(rawNote);

      const sound = valueAt(value.s !== undefined ? value.s : value.sound, cycle);
      const percussion = drumPreset(sound);
      if (percussion) {
        if (note === null) note = percussion.note;
        waveform = waveformCode(percussion.wave);
        lpf = percussion.lpf;
        shape = percussion.shape;
        sourceId = 100 + waveform;
      } else if (sound !== undefined) {
        const namedWave = waveformCode(sound);
        if (["sine", "square", "saw", "sawtooth", "triangle", "noise", "white", "pulse"].includes(String(sound).toLowerCase())) {
          waveform = namedWave;
          sourceId = 100 + namedWave;
        }
      }
      const bank = valueAt(value.bank, cycle);
      const sampleName = percussion && String(sound).toLowerCase().split(":")[0];
      if (String(bank || "").toLowerCase() === "trueos" && sampleName && ["bd", "hh", "sd"].includes(sampleName)) {
        kind = 2;
        sourceId = sampleSourceId("trueos", sampleName);
      }

      if (value.velocity !== undefined) velocity = valueAt(value.velocity, cycle);
      else if (value.vel !== undefined) velocity = valueAt(value.vel, cycle);
      else if (value.gain !== undefined) velocity = Number(valueAt(value.gain, cycle)) * 127;
      if (value.postgain !== undefined) velocity *= Number(valueAt(value.postgain, cycle));

      if (value.wave !== undefined || value.waveform !== undefined) {
        waveform = waveformCode(valueAt(value.wave !== undefined ? value.wave : value.waveform, cycle));
      }
      if (value.pan !== undefined) pan = Number(valueAt(value.pan, cycle));
      if (value.lpf !== undefined) lpf = Number(valueAt(value.lpf, cycle));
      // Native renderer has a low-pass, not a band-pass. The BPF center is
      // therefore represented by its cutoff, preserving a useful timbral cue.
      if (value.bpf !== undefined && !value.lpf) lpf = Number(valueAt(value.bpf, cycle));
      if (value.lpq !== undefined) lpq = Number(valueAt(value.lpq, cycle));
      if (value.room !== undefined) room = Number(valueAt(value.room, cycle));
      if (value.delay !== undefined) delay = Number(valueAt(value.delay, cycle));
      if (value.phaser !== undefined) phaser = Number(valueAt(value.phaser, cycle));
      if (value.shape !== undefined) shape = Number(valueAt(value.shape, cycle));
      if (value.fm !== undefined) fm = Number(valueAt(value.fm, cycle));
      if (value.fmRate !== undefined) fmRate = Number(valueAt(value.fmRate, cycle));
      if (value.attack !== undefined) attack = Number(valueAt(value.attack, cycle));
      if (value.decay !== undefined) decay = Number(valueAt(value.decay, cycle));
      if (value.sustain !== undefined) sustain = Number(valueAt(value.sustain, cycle));
      if (value.release !== undefined) release = Number(valueAt(value.release, cycle));
      if (value.lpa !== undefined) filterAttack = Number(valueAt(value.lpa, cycle));
      if (value.lpd !== undefined) filterDecay = Number(valueAt(value.lpd, cycle));
      if (value.lpenv !== undefined) filterEnvOctaves = Number(valueAt(value.lpenv, cycle));
      if (value.ftype !== undefined) filterType = filterTypeCode(valueAt(value.ftype, cycle));
      if (value.clip !== undefined) clip = Number(valueAt(value.clip, cycle));
      if (value.add !== undefined) {
        const additive = valueAt(value.add, cycle);
        const addition = additive && typeof additive === "object" ? additive.note : additive;
        const delta = noteNameToMidi(addition);
        if (delta !== null && note !== null) note = clampInteger(note + delta, 0, 127);
      }
    }

    if (note === null) return null;
    const releaseFrames = Math.min(
      secondsToFrames(release, sampleRate),
      sampleRate * 10,
    );
    releaseLookbackFrames = Math.max(releaseLookbackFrames, releaseFrames);
    return {
      note,
      velocity: clampInteger(velocity, 0, 127),
      waveform,
      panQ15: clampInteger(Math.max(-1, Math.min(1, pan)) * 32767, -32768, 32767),
      sourceId,
      lpf: clampInteger(lpf, 0, 24000),
      lpqQ8: clampInteger(lpq * 256, 0, 65535),
      roomQ15: clampInteger(clamp(room, 0, 1) * 32767, 0, 32767),
      delayQ15: clampInteger(clamp(delay, 0, 1) * 32767, 0, 32767),
      phaserQ15: clampInteger(clamp(phaser / 8, 0, 1) * 32767, 0, 32767),
      shapeQ15: clampInteger(clamp(shape, 0, 1) * 32767, 0, 32767),
      fmDepthQ8: clampInteger(Math.max(0, fm) * 256, 0, 65535),
      fmRateQ8: clampInteger(Math.max(0, fmRate) * 256, 0, 65535),
      kind,
      sampleBegin,
      sampleEnd,
      flags,
      attackFrames: secondsToFrames(attack, sampleRate),
      decayFrames: secondsToFrames(decay, sampleRate),
      sustainQ15: clampInteger(clamp(sustain, 0, 1) * 32767, 0, 32767),
      releaseFrames,
      filterAttackFrames: secondsToFrames(filterAttack, sampleRate),
      filterDecayFrames: secondsToFrames(filterDecay, sampleRate),
      // Kernel V2 validates a bounded ±8 octave sweep; constrain before the
      // VM boundary so valid editor code never turns into a host-side EINVAL.
      filterEnvOctavesQ8: clampInteger(clamp(filterEnvOctaves, -8, 8) * 256, -2048, 2048),
      filterType,
      clip: Number.isFinite(clip) ? clip : 1,
    };
  }

  function secondsToFrames(value, sampleRate) {
    value = Number(value);
    if (!Number.isFinite(value) || value <= 0) return 0;
    // A bounded integer frame field prevents a malformed editor program from
    // creating a practically immortal native voice.
    return clampInteger(value * sampleRate, 0, sampleRate * 600);
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
        // Indirect eval executes in global scope. JavaScript's completion value
        // lets normal Strudel programs use `setcps(1); stack(...)` as well as a
        // single expression, while acceptPattern below keeps the commit atomic.
        candidate = globalEval(source);
      } catch (error) {
        throw new Error(`pattern program failed: ${describeError(error)}`);
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
    // Pattern queries normally stop returning an event at its gate edge. A
    // positive `.clip()` can extend that edge, so retain the bounded maximum
    // native gate extension as well as the V3 release tail.
    const releaseLookbackCycles = (Math.max(releaseLookbackFrames, sampleRate * 10) / sampleRate) * cps;
    const haps = pattern.queryArc(cycleBegin - releaseLookbackCycles, cycleEnd);
    const rows = [];

    for (const hap of haps) {
      const whole = hap.whole || hap.part;
      if (!whole) continue;
      const wholeBegin = finiteNumber(whole.begin);
      const wholeEnd = finiteNumber(whole.end);
      if (!Number.isFinite(wholeBegin) || !Number.isFinite(wholeEnd) || wholeEnd <= wholeBegin) {
        continue;
      }

      const voice = voiceFromValue(hap.value, wholeBegin, sampleRate);
      if (!voice || voice.velocity === 0) continue;

      const onsetFrame = Math.round((wholeBegin / cps) * sampleRate);
      // Upstream clip scales an event's gate; samples are cut at that gate and
      // the native release begins there, rather than at the unmodified span.
      if (!(voice.clip > 0)) continue;
      const gateFrames = Math.max(1, Math.round(((wholeEnd - wholeBegin) / cps) * sampleRate * voice.clip));
      const releaseFrame = onsetFrame + gateFrames;
      const clippedStart = Math.max(absoluteStartFrame, onsetFrame);
      // Gate duration deliberately excludes the release tail. V2's end span
      // includes it, allowing native ADSR to drain without a browser reset.
      const tailEndFrame = releaseFrame + voice.releaseFrames;
      const clippedEnd = Math.min(absoluteEndFrame, tailEndFrame);
      if (clippedEnd <= clippedStart) continue;

      const gainQ15 = clampInteger((voice.velocity / 127) * 32767, 0, 32767);
      // Stable across lookahead blocks, distinct for overlapping notes that
      // share one oscillator/sample source. The host uses this identity to
      // retain per-voice filter state without joining separate notes.
      const voiceId = (
        (onsetFrame >>> 0)
        ^ ((voice.note & 127) << 24)
        ^ (Math.trunc(voice.sourceId) >>> 0)
      ) >>> 0;
      rows.push([
        clippedStart - absoluteStartFrame,
        clippedEnd - absoluteStartFrame,
        Math.max(0, clippedStart - onsetFrame),
        gateFrames,
        voice.sourceId,
        voiceId,
        voice.kind,
        voice.waveform,
        voice.note,
        gainQ15,
        voice.panQ15,
        65536,
        voice.sampleBegin,
        voice.sampleEnd,
        voice.lpf,
        voice.lpqQ8,
        voice.roomQ15,
        voice.delayQ15,
        voice.phaserQ15,
        voice.shapeQ15,
        voice.fmDepthQ8,
        voice.fmRateQ8,
        voice.flags,
        voice.attackFrames,
        voice.decayFrames,
        voice.releaseFrames,
        voice.filterAttackFrames,
        voice.filterDecayFrames,
        voice.sustainQ15,
        voice.filterEnvOctavesQ8,
        voice.filterType,
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
