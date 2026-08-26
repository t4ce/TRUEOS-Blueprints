/*
 * TRUEOS instrument vocabulary.
 *
 * This is deliberately a data file, not a second synthesizer.  The same
 * entries are available to the QuickJS adapter and can be serialized by the
 * HTTP/Monaco side for a read-only instrument palette.  Presets describe a
 * useful first sound using the native oscillator fields; they do not pretend
 * to be recordings of the named instruments.
 */
(function installTrueosInstrumentCatalog(G) {
  "use strict";

  const entries = [
    { id: "drums", label: "Drums", icon: "🥁", family: "percussion", wave: "noise", gain: 0.9, pan: 0, lpf: 2400, room: 0.08, shape: 0.15, snippet: 'instrument("drums")' },
    { id: "piano", label: "Piano", icon: "🎹", family: "keys", wave: "triangle", gain: 0.78, pan: 0, lpf: 5200, room: 0.12, delay: 0.08, shape: 0.12, snippet: 'instrument("piano")' },
    { id: "guitar", label: "Guitar", icon: "🎸", family: "strings", wave: "triangle", gain: 0.72, pan: 0, lpf: 3200, room: 0.16, delay: 0.1, shape: 0.25, snippet: 'instrument("guitar")' },
    { id: "sax", label: "Saxophone", icon: "🎷", family: "winds", wave: "saw", gain: 0.7, pan: 0, lpf: 1800, room: 0.2, shape: 0.28, fm: 0.08, snippet: 'instrument("sax")' },
    { id: "trumpet", label: "Trumpet", icon: "🎺", family: "brass", wave: "saw", gain: 0.68, pan: 0, lpf: 2300, room: 0.14, shape: 0.34, fm: 0.05, snippet: 'instrument("trumpet")' },
    { id: "violin", label: "Violin", icon: "🎻", family: "strings", wave: "saw", gain: 0.64, pan: 0, lpf: 3400, room: 0.22, shape: 0.2, fm: 0.12, snippet: 'instrument("violin")' },
    { id: "maracas", label: "Maracas", icon: "🪇", family: "percussion", wave: "noise", gain: 0.58, pan: 0, lpf: 6500, room: 0.1, shape: 0.5, snippet: 'instrument("maracas")' },
    { id: "flute", label: "Flute", icon: "🪈", family: "winds", wave: "sine", gain: 0.62, pan: 0, lpf: 3600, room: 0.18, shape: 0.08, fm: 0.04, snippet: 'instrument("flute")' },
    { id: "banjo", label: "Banjo", icon: "🪕", family: "strings", wave: "triangle", gain: 0.7, pan: 0, lpf: 4300, room: 0.1, delay: 0.06, shape: 0.32, snippet: 'instrument("banjo")' },
    { id: "accordion", label: "Accordion", icon: "🪗", family: "keys", wave: "square", gain: 0.62, pan: 0, lpf: 2100, room: 0.16, shape: 0.18, fm: 0.1, snippet: 'instrument("accordion")' },
    { id: "conga", label: "Conga", icon: "🪘", family: "percussion", wave: "sine", gain: 0.82, pan: 0, lpf: 900, room: 0.12, shape: 0.34, snippet: 'instrument("conga")' },
    { id: "voice", label: "Voice", icon: "🎤", family: "voice", wave: "saw", gain: 0.62, pan: 0, lpf: 1900, room: 0.2, shape: 0.16, fm: 0.06, snippet: 'instrument("voice")' },
    { id: "bass", label: "Bass", icon: "🎚️", family: "low", wave: "square", gain: 0.8, pan: 0, lpf: 700, room: 0.08, shape: 0.2, snippet: 'instrument("bass")' },
  ];

  const byId = Object.create(null);
  for (const entry of entries) byId[entry.id] = Object.freeze(entry);
  const catalog = Object.freeze({ entries: Object.freeze(entries.map((entry) => byId[entry.id])), byId: Object.freeze(byId) });

  Object.defineProperty(G, "__TRUEOS_INSTRUMENT_CATALOG", {
    value: catalog,
    writable: false,
    configurable: false,
    enumerable: false,
  });
})(globalThis);
