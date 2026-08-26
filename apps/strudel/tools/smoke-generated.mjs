import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const toolDir = dirname(fileURLToPath(import.meta.url));
const bundle = resolve(
  process.env.TRUEOS_STRUDEL_BUNDLE ||
    join(toolDir, '..', 'apps', 'strudel_core', 'js', 'vendor', 'strudel-core.bundle.js'),
);
if (!existsSync(bundle)) throw new Error(`bundle missing: ${bundle}`);

delete globalThis.StrudelCore;
await import(`${pathToFileURL(bundle).href}?smoke=${Date.now()}`);
const S = globalThis.StrudelCore;
if (!S) throw new Error('bundle did not install globalThis.StrudelCore');

const haps = S.sequence('a', ['b', 'c']).queryArc(0, 1);
const observed = haps.map((hap) => [
  hap.value,
  Number((hap.whole || hap.part).begin),
  Number((hap.whole || hap.part).end),
]);
const expected = [
  ['a', 0, 0.5],
  ['b', 0.5, 0.75],
  ['c', 0.75, 1],
];
if (JSON.stringify(observed) !== JSON.stringify(expected)) {
  throw new Error(`upstream smoke mismatch: ${JSON.stringify(observed)}`);
}
