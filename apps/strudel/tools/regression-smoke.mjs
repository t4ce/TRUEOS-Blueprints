import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const toolDir = dirname(fileURLToPath(import.meta.url));
const appJs = join(toolDir, '..', 'apps', 'strudel_core', 'js');
const context = vm.createContext({ console, globalThis: null });
context.globalThis = context;

for (const file of ['00_fallback_core.js', '10_trueos_adapter.js']) {
  vm.runInContext(await readFile(join(appJs, file), 'utf8'), context, { filename: file });
}

vm.runInContext(
  `globalThis.__TRUEOS_STRUDEL.setPattern(
    globalThis.StrudelCoreFallback.sequence(
      { note: 60, velocity: 100 },
      [{ note: 61, velocity: 100 }, { note: 62, velocity: 100 }]
    )
  )`,
  context,
);

const sampleRate = 48_000;
const blockFrames = 2_400;
const totalFrames = 96_000; // one cycle at 0.5 cps
const rows = [];
for (let absolute = 0; absolute < totalFrames; absolute += blockFrames) {
  const blockRows = vm.runInContext(
    `globalThis.__TRUEOS_STRUDEL.queryFrames(${absolute},${blockFrames},${sampleRate},1,2)`,
    context,
  );
  for (const row of blockRows) {
    const [start, end, age, duration, note] = row;
    rows.push({
      note,
      globalStart: absolute + start,
      globalEnd: absolute + end,
      age,
      duration,
    });
  }
}

const expected = new Map([
  [60, { onset: 0, release: 48_000 }],
  [61, { onset: 48_000, release: 72_000 }],
  [62, { onset: 72_000, release: 96_000 }],
]);

for (const [note, span] of expected) {
  const voiceRows = rows.filter((row) => row.note === note);
  if (voiceRows.length === 0) throw new Error(`no rows for note ${note}`);
  if (voiceRows[0].globalStart !== span.onset) {
    throw new Error(`note ${note} onset mismatch: ${voiceRows[0].globalStart}`);
  }
  if (voiceRows.at(-1).globalEnd !== span.release) {
    throw new Error(`note ${note} release mismatch: ${voiceRows.at(-1).globalEnd}`);
  }
  for (let index = 0; index < voiceRows.length; index += 1) {
    const row = voiceRows[index];
    if (row.age !== row.globalStart - span.onset) {
      throw new Error(`note ${note} age mismatch: ${JSON.stringify(row)}`);
    }
    if (row.duration !== span.release - span.onset) {
      throw new Error(`note ${note} duration mismatch: ${JSON.stringify(row)}`);
    }
    if (index > 0 && voiceRows[index - 1].globalEnd !== row.globalStart) {
      throw new Error(`note ${note} has a block gap/overlap at ${row.globalStart}`);
    }
  }
}

console.log(
  JSON.stringify(
    {
      blocks: totalFrames / blockFrames,
      rows: rows.length,
      spans: Object.fromEntries(
        [...expected].map(([note, span]) => [note, `${span.onset}-${span.release}`]),
      ),
    },
    null,
    2,
  ),
);
