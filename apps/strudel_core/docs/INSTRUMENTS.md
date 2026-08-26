# Instrument vocabulary

The embedded Strudel runtime is the pattern/notation layer, not the browser
audio layer.  It deliberately does not export `s()`, `n()`, `note()`,
`samples()`, `sound()`, `chord()`, or `setcps()` from upstream.  `setcps()` is
installed by the TRUEOS adapter, while the audio-free bundle supplies
`m()`, `mini()`, `sequence()`, `stack()`, `struct()`, tonal transforms, and the
temporal Pattern methods.

Consequently an instrument is currently an ordinary event value.  This is a
useful stable spelling for the native renderer and for a future read-only
instrument pane:

```js
sequence(
  { instrument: "drums", note: 36, velocity: 110 },
  { instrument: "piano", note: "c4", velocity: 88 },
  { instrument: "bass", note: "c2", velocity: 96 }
)
```

The adapter consumes `note`/`midinote`/`midi`/`n`, `velocity` (or
`vel`/`gain`), `wave`/`waveform`, and `pan`. `instrument(name, options)` applies
the catalog's oscillator defaults before explicit options override them, so
the names are audible native presets rather than comments. They are not sample
banks: recorded sample playback still requires registered PCM and an explicit
native sample command.

The initial catalog vocabulary is:

| family | names | icon |
| --- | --- | --- |
| percussion | drums, maracas, conga | 🥁 🪇 🪘 |
| keyboard/strings | piano, accordion, guitar, banjo | 🎹 🎸 🪗 🪕 |
| winds | sax, trumpet, flute | 🎷 🎺 🪈 |
| bowed/voice | violin, voice | 🎻 🎤 |

These names are labels, not upstream Strudel functions.  For temporal
composition use the real Pattern operations: `.palindrome()`, `.every()`,
`.timecat()`, `.iter()`, `.chunk()`, `.fast()`, `.slow()`, `.struct()`,
`.scale()`, `.transpose()`, and `.voicing()`.  Mini notation is available via
`m("c4 <e4 g4>")`; `m("...").scale("C:major")` and tonal transforms are also
audio-free and run in QuickJS.

The following common Strudel examples remain outside this bundle until the
corresponding native features land: `samples(...)`, `.bank(...)`, `.s(...)`,
`.room()`, `.delay()`, `.phaser()`, `.fm()`, `.lpf()`, `.clip()`, and live
`rand`/`perlin` signal controls.  They can be represented as metadata for the
readonly editor/catalog, but should not be advertised as executable audio
syntax yet.
