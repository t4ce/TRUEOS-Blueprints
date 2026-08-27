import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const jsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "js");
for (const file of [
  "vendor/strudel-core.bundle.js",
  "00_fallback_core.js",
  "instrument_catalog.js",
  "10_trueos_adapter.js",
]) {
  vm.runInThisContext(await readFile(join(jsDir, file), "utf8"), { filename: file });
}

for (const name of ["sound", "s", "n", "setcpm", "setcps"]) {
  if (typeof globalThis[name] !== "function") throw new Error(`workshop global missing: ${name}`);
}

const examples = [
  'sound("casio")',
  'sound("insect wind jazz metal east crow casio space numbers")',
  'sound("casio:1")',
  'sound("bd hh sd oh")',
  'sound("bd hh sd oh").bank("RolandTR909")',
  'sound("bd hh sd hh")',
  'sound("bd bd hh bd rim bd hh bd")',
  'sound("<bd bd hh bd rim bd hh bd>")',
  'sound("<bd bd hh bd rim bd hh bd>*8")',
  'setcpm(90/4); sound("<bd hh rim hh>*8")',
  'sound("bd hh - rim - bd hh rim")',
  'sound("bd [hh hh] sd [hh bd] bd - [hh sd] cp")',
  'sound("bd hh*2 rim hh*3 bd [- hh*2] rim hh*2")',
  'sound("bd [hh rim]*2 bd [hh rim]*1.5")',
  'sound("bd hh*32 rim hh*16")',
  'sound("bd [[rim rim] hh] bd cp")',
  'sound("hh hh hh, bd casio")',
  'sound("hh hh hh, bd bd, - casio")',
  'sound("hh hh hh, bd [bd,casio]")',
  'sound(`bd*2, - cp,\n- - - oh, hh*4,\n[- casio]*2`)',
  'sound("jazz:0 jazz:1 [jazz:4 jazz:2] jazz:3*2")',
  'n("0 1 [4 2] 3*2").sound("jazz")',
  'setcpm(100/4); sound("[bd sd]*2, hh*8").bank("RolandTR505")',
  'sound("bd*4, [- cp]*2, [- hh]*4").bank("RolandTR909")',
  'setcpm(81/2); sound("bd*2 cp").bank("RolandTR707")',
  'setcpm(120/2); sound("bd sd, - - - hh - hh - -, - perc - perc:1*2").bank("RolandCompurhythm1000")',
  'setcpm(100/2); s(`jazz*2,\ninsect [crow metal] - -,\n- space:4 - space:1,\n- wind`)',
];

const runtime = globalThis.__TRUEOS_STRUDEL;
for (const [index, source] of examples.entries()) {
  let status;
  try {
    status = runtime.commitExpression(`setcps(.5); ${source}`);
  } catch (error) {
    throw new Error(`workshop example ${index + 1} rejected: ${source}\n${error.stack || error}`);
  }
  const cps = status.cpsNumerator / status.cpsDenominator;
  const fourCycles = Math.ceil((4 * 48_000) / cps);
  const rows = runtime.queryFrames(0, fourCycles, 48_000);
  if (!rows.length) throw new Error(`workshop example ${index + 1} emitted no native voices: ${source}`);
}

runtime.commitExpression('setcps(.5); sound("casio")');
if (!runtime.queryFrames(0, 96_000, 48_000).some((row) => row[8] === 60)) {
  throw new Error("casio compatibility voice was not rendered");
}

runtime.commitExpression('setcps(.5); n("0 1 [4 2] 3*2").sound("jazz")');
const jazzNotes = new Set(runtime.queryFrames(0, 96_000, 48_000).map((row) => row[8]));
if (![57, 58, 61, 59, 60].some((note) => jazzNotes.has(note))) {
  throw new Error(`jazz sample-number selection was not rendered: ${JSON.stringify([...jazzNotes])}`);
}

console.log("strudel_core first-sounds workshop smoke passed");
