import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const toolDir = dirname(fileURLToPath(import.meta.url));
const appJs = join(toolDir, '..', 'apps', 'strudel_core', 'js');
const context = vm.createContext({ console, globalThis: null });
context.globalThis = context;

for (const file of ['00_fallback_core.js', '10_trueos_adapter.js', '20_demo_pattern.js']) {
  vm.runInContext(await readFile(join(appJs, file), 'utf8'), context, { filename: file });
}

const observed = vm.runInContext('globalThis.__TRUEOS_STRUDEL.selfTest()', context);
const expected = 'a@0.000000-0.500000|b@0.500000-0.750000|c@0.750000-1.000000';
if (observed !== expected) throw new Error(`fallback temporal mismatch: ${observed}`);

const rows = vm.runInContext(
  'globalThis.__TRUEOS_STRUDEL.queryFrames(0, 2400, 48000, 1, 2)',
  context,
);
if (!Array.isArray(rows) || rows.length === 0) throw new Error('demo emitted no rows');
for (const row of rows) {
  if (!Array.isArray(row) || row.length !== 8 || row.some((value) => !Number.isInteger(value))) {
    throw new Error(`bad event row: ${JSON.stringify(row)}`);
  }
}
console.log(JSON.stringify({ temporal: observed, firstBlockRows: rows }, null, 2));
