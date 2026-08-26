import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = join(dirname(fileURLToPath(import.meta.url)), '..');
for (const file of ['js/vendor/strudel-core.bundle.js', 'js/00_fallback_core.js', 'js/10_trueos_adapter.js']) {
  vm.runInThisContext(await readFile(join(app, file), 'utf8'), { filename: file });
}

const { StrudelCore: core, __TRUEOS_STRUDEL: bridge } = globalThis;
if (!core || bridge.status().source !== 'upstream') throw new Error('upstream bundle was not selected');
for (const method of ['palindrome', 'every', 'timecat', 'iter', 'chunk', 'm', 'scale', 'transpose', 'voicing']) {
  if (typeof core[method] !== 'function') throw new Error(`missing upstream export: ${method}`);
}

const haps = (pattern) => pattern.queryArc(0, 2).map((hap) => hap.value);
if (haps(core.sequence('a', 'b').palindrome()).length === 0) throw new Error('palindrome did not query');
if (haps(core.sequence('a', 'b').every(2, (p) => p.rev())).length === 0) throw new Error('every did not query');
if (haps(core.timecat([1, core.sequence('a')], [1, core.sequence('b')])).join(',') !== 'a,a,b,b') throw new Error('timecat mismatch');
if (haps(core.sequence('a', 'b', 'c', 'd').iter(2)).length === 0) throw new Error('iter did not query');
if (haps(core.sequence('a', 'b', 'c', 'd').chunk(2, (p) => p.rev())).length === 0) throw new Error('chunk did not query');
if (core.m('c4 e4 g4').queryArc(0, 1).map((hap) => hap.value).join(',') !== 'c4,e4,g4') throw new Error('mini notation mismatch');
if (core.m('0 2 4').scale('C:major').queryArc(0, 1).map((hap) => hap.value).join(',') !== 'C3,E3,G3') throw new Error('tonal scale mismatch');
if (core.m('c4').transpose('3M').queryArc(0, 1).map((hap) => hap.value).join(',') !== 'E4') throw new Error('tonal transpose mismatch');

console.log('strudel_core upstream vendor smoke passed');
