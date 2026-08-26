import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = join(dirname(fileURLToPath(import.meta.url)), '..');
for (const file of ['js/vendor/strudel-core.bundle.js', 'js/00_fallback_core.js', 'js/instrument_catalog.js', 'js/10_trueos_adapter.js']) {
  vm.runInThisContext(await readFile(join(app, file), 'utf8'), { filename: file });
}

const { StrudelCore: core, __TRUEOS_STRUDEL: bridge } = globalThis;
if (!core || bridge.status().source !== 'upstream') throw new Error('upstream runtime missing');

const catalog = ['drums', 'piano', 'guitar', 'bass', 'sax', 'trumpet', 'violin', 'flute', 'banjo', 'accordion', 'maracas', 'conga', 'voice'];
if (globalThis.instrument('🎚️', { note: 'c2' }).instrument !== 'bass') {
  throw new Error('instrument icon alias did not resolve');
}
const pattern = core.stack(
  core.sequence(...catalog.map((instrument, index) => ({ instrument, note: 48 + index, velocity: 80 }))),
  core.m('0 2 4').scale('C:major'),
).palindrome().every(2, (p) => p.fast(2));

const values = pattern.queryArc(0, 2).map((hap) => hap.value);
if (values.length < catalog.length) throw new Error('instrument metadata did not survive Pattern transforms');
if (!values.some((value) => value && value.instrument === 'piano')) throw new Error('piano metadata missing');
if (!values.some((value) => value === 'C3' || value === 'E3' || value === 'G3')) throw new Error('tonal Pattern transform missing');

for (const name of ['s', 'note', 'add']) {
  if (typeof globalThis[name] !== 'function') throw new Error(`${name} shorthand is missing`);
}
for (const name of ['samples', 'sound']) {
  if (typeof globalThis[name] === 'function') throw new Error(`${name} unexpectedly claims sample support`);
}

console.log('instrument vocabulary smoke passed');
