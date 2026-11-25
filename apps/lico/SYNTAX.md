# Lico patch syntax

## Chains, routing, and levels

A patch defines named chains. Write `name: node >> node` and end the chain
with a newline or semicolon. A continued line starts with `>>`. Use `//`
for comments. Names use lowercase letters, digits, and underscores, with a
letter first after any `~` or `_` prefix. `~🥁` is an alias for `~drum`.
Numbered drum names such as `~🥁2` are also valid private chains.

Chains that start with `~` only play when routed into another chain. Other
chains go to the main mix, so `out` is a conventional name for the final
chain. Use `get ~name` or just `~name` to start from the signal at the end
of a named chain. A reference can appear before its definition.

```glicol
// Split the mix into private voice chains and one audible output.
~voice1: tri 220 >> mul 0.12;
~voice2: sin 330 >> mul 0.08;
out: mix ~voice..
>> pan 0.2;
```

| Node | Arguments and behavior |
| --- | --- |
| `get` | One reference. A bare reference has the same meaning. |
| `mix` | One or more references. `~voice..` matches all chains beginning with `~voice`. |
| `mul`, `add` | A number or reference. Multiply the input by a gain/modulator or add an offset/signal. |
| `pan` | A position or reference between −1 (left) and +1 (right), with 0 centered. |
| `balance` | Two references. Route the first signal to the left channel and the second to the right. |

Arguments are generally separated by spaces. Write fractions as `0.5`
with a leading digit, and use a leading sign for negative values. A slash
fraction such as `1/4` is available in points syntax.

## Rhythm, samples, and pitched voices

```glicol
~voice: speed 2 >> seq 48 _55 60_62 _ >> trisynth 0.004 0.12;
~kick: seq 60 _ 60 _ >> sp \808bd;
~hat: seq _ 60 _ 60 >> sp \808ch;
out: mix ~voice ~kick ~hat >> mul 0.3;
```

`seq` divides one bar into space-separated slots at the default speed.
Within a slot, adjacent notes and rests subdivide it. In `60_62`, note 60 plays on the first third
of the slot, the middle third is a rest, and note 62 plays on the last
third. A lone `_` rests for a whole slot. A note of 60 emits a trigger
value of 1.0, 72 emits 2.0, and 48 emits 0.5. Between events the output is
zero. Use a single-letter chain such as `~a: choose 48 55 60` with
`seq 60 _ ~a _` to select pitches from a list. A choice of 0 makes a rest.

| Node | Arguments and behavior |
| --- | --- |
| `speed` | A positive cycle speed multiplier. Put it before `seq`. A multiplier of 2 fits two cycles into one bar. |
| `seq` | Nonnegative MIDI notes, rests, and single-letter references, grouped into slots. |
| `choose` | Space-separated integers. Emits one randomly selected value per processing block. |
| `sp`, `sampler` | A bundled sample symbol such as `\808ch`. A positive trigger starts playback, with 1.0 at native pitch. |
| `psampler` | A quoted repeating series of `sample@bar_position` pairs followed by a period in bars. |
| `bd`, `sn`, `hh` | Decay in seconds. Trigger synthesized kick, snare, and hi-hat sounds with a positive pulse. |
| `sawsynth`, `squsynth`, `trisynth` | Attack and decay in seconds. Each node turns incoming pitch triggers into a short saw, square, or triangle voice. |
| `psynth`, `p_synth`, `pattern_synth` | Backtick-delimited `position midi` pairs separated by commas, followed by a number. Multiple notes at one position form a chord. |
| `arrange` | Alternating reference and length in bars. Plays the input sections in order and wraps to the first. |

The bundled samples include `\808bd`, `\808sd`, `\808ch`, and `\blip`.
The acoustic kit adds `\kit_kick`, `\kit_snare`, `\kit_tom_high`,
`\kit_tom_mid`, `\kit_tom_floor`, `\kit_hat_closed`, `\kit_hat_pedal`,
`\kit_hat_open`, `\kit_crash`, and `\kit_ride`.
`psampler "\808bd@0 \808ch@0.25 \808sd@0.5 \808ch@0.75"(1)` plays four
events over one bar. A suffix of `()` also uses one bar. Keep events
within the period, starting at zero and excluding the end of the loop.

For section changes, use `out: arrange ~verse 2 ~chorus 2`. For a
short polyphonic pattern, use
``out: psynth `0 60, 0 64, 0 67, 0.5 62, 0.5 65, 0.5 69` 1``.
Lico's bundled engine currently runs `psynth` on a fixed two-second
cycle. The trailing number is required by the parser, and the engine
uses its default cycle length.

## Modulation, envelopes, and filters

```glicol
~gate: sin 0.5 >> eval `in > 0.0`;
~shape: ~gate >> adsr 0.03 0.12 0.5 0.2;
~cutoff: [0=>250 1/4=>1400 1/2=>700 1=>250]!;
out: saw 110 >> lpf ~cutoff 1.2 >> mul ~shape >> mul 0.18;
```

| Node | Arguments and behavior |
| --- | --- |
| `sin`, `saw`, `squ`, `tri` | Frequency in Hz, as a number or an audio/control reference. |
| `sig`, `constsig` | One number. Generate a constant control signal. |
| `imp` | A numeric rate in Hz. Generate pulses for envelopes and other trigger inputs. |
| `noise`, `noiz` | A nonnegative integer seed. Generate white noise. |
| `envperc` | Attack and decay in seconds. A pulse sets the envelope in motion. |
| `adsr` | Attack and decay in seconds, a sustain level from 0 to 1, and release in seconds. A positive gate starts the envelope and zero releases it. |
| `lpf` | Cutoff in Hz, followed by numeric Q. The cutoff can be a number, reference, or repeating numeric pattern such as `"300@0 1200@0.5"(1)`. |
| `hpf`, `rhpf` | Cutoff in Hz as a number or reference, followed by numeric Q. |
| `onepole` | A normalized filter coefficient parameter, as a number or reference. For example, `onepole 0.02` smooths the input. |
| `[time=>value ...]` | A control curve. Positions are in bars, with optional offsets in seconds or milliseconds, such as `1/4+20_ms=>0.5`. |

Use `!` after a points list for a repeating curve. Add a multiplier or
divisor before `!` to change its period, such as
`[0=>300 1/2=>1200 1=>300] * 2 !` for two bars. Supply at least two
points in ascending order with a positive cycle length.

## Delay, reverb, and custom DSP

```glicol
~dry: seq 60 _ _ _ >> sp \blip >> mul 0.3;
~echo: ~dry >> delayms 180 >> mul 0.5;
out: mix ~dry ~echo >> plate 0.12;
```

| Node | Arguments and behavior |
| --- | --- |
| `delayms` | Delay in milliseconds, as a number or reference. Mix a dry and delayed route for an echo. |
| `delayn` | A fixed nonnegative integer sample count. The parser also accepts a reference, but this engine needs a fixed count for the delay to run. |
| `apfmsgain`, `apfgain` | All-pass delay in milliseconds as a number or reference, followed by numeric feedback gain. |
| `plate` | Reverb mix from 0 to 1. |
| `eval` | Backtick-delimited per-sample expression. It accepts `name := expression;` assignments followed by a final expression. `in` is the input sample, `phase` is the sample counter, and `sr` is the sample rate. |
| `meta`, `script` | Backtick-delimited Rhai script. With an upstream chain, `input` contains its first channel. Return an array of 128 samples. |

For example, `` eval `wet := in * in; wet * 0.5` `` transforms one
sample at a time. `` meta `input.map(|x| x - 0.45 * x * x * x)` ``
waveshapes the block.

## Live MIDI instrument

```glicol
out: midisynth 0 0.003 0.16 piano;
```

`midisynth CHANNEL ATTACK RELEASE [piano]` generates up to eight simultaneous
voices. Choose channel `1` through `16`, or `0` for all channels. Attack
and release are in seconds. The optional `piano` voice synthesizes the
strike and string decay for each key. MIDI velocity controls its level and
brightness. The default sine voice holds its level until Note Off or All
Notes Off. Connect the output to filters, effects, and other Glicol nodes
like any other signal.

Run the patch with **Alt+R**, then press **Alt+M** to send Lico’s channel 1
test phrase through its MIDI byte decoder.

```glicol
~piano: midisynth 0 0.003 0.16 piano;
~answer: midiarp 0 0.25 7 12 7 >> trisynth 0.004 0.085 >> mul 0.015;
out: mix ~piano ~answer;
```

`midiarp CHANNEL STEP_BEATS OFFSET ...` starts a one-shot phrase from each
matching Note On. Channel `0` listens to every channel. The positive step
length is measured in beats and the first step waits that long after key-down.
Offsets are signed semitones relative to the pressed key. Each step sends a
`seq`-style pitch pulse to the next node. A new key restarts the phrase from
its own pitch. Releasing a key lets its phrase finish.

## Forms outside the Lico playback workflow

- `msgsynth` (also `msg_synth`) parses a sample-style symbol, attack, and
  decay. The synth needs note messages from a host, and Lico's editor and
  Codex connection currently supply no note input for it.
- The grammar includes `adc`, `reverb`, `expr`, and `sendpass`. Those
  forms have no playable Lico implementation. Use `plate` for built-in
  reverb.
- The grammar accepts a bare quoted event list for `psampler` and as a
  cutoff for `lpf`. Lico playback requires the repeating pattern form
  with the parentheses after the closing quote.
