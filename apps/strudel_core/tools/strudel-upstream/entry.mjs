// Intentionally audio-free: do not import @strudel/core's index.mjs.
// The selected modules are the temporal Pattern engine, mini notation, and
// tonal/chord voicing transforms used by the text evaluator.
import * as temporal from '@strudel/core/pattern.mjs';
import * as mini from '@strudel/mini/mini.mjs';
import * as tonal from '@strudel/tonal/tonal.mjs';
import * as voicings from '@strudel/tonal/voicings.mjs';

const StrudelCore = Object.freeze({ ...temporal, ...mini, ...tonal, ...voicings });
globalThis.StrudelCore = StrudelCore;
globalThis.__TRUEOS_UPSTREAM_STRUDEL_PRESENT = true;
globalThis.__TRUEOS_UPSTREAM_STRUDEL_VERSION = '1.2.6';
globalThis.__TRUEOS_UPSTREAM_STRUDEL_ORIGIN = 'npm:@strudel/core,@strudel/mini,@strudel/tonal';
