import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const jsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "js");
for (const file of ["vendor/strudel-core.bundle.js", "00_fallback_core.js", "instrument_catalog.js", "10_trueos_adapter.js"]) {
  vm.runInThisContext(await readFile(join(jsDir, file), "utf8"), { filename: file });
}
const runtime = globalThis.__TRUEOS_STRUDEL;
function onsets(rows, absoluteStart) {
  return rows.filter((row) => row[2] === 0).map((row) => [absoluteStart + row[0], row[4], row[5], row[8]]);
}
function rowsFor(source, start = 0, length = 48000) {
  runtime.commitExpression(`setcps(1); ${source}`);
  return runtime.queryFrames(start, length, 48000);
}

// The two masks used by the live shorthand program suppress a proper subset.
const bdFull = rowsFor('s("bd*2")');
const bdMasked = rowsFor('s("bd*2").mask("<0@4 1@16>")');
if (!(bdMasked.length > 0 && bdMasked.length < bdFull.length)) throw new Error(`bd mask: ${bdFull.length}/${bdMasked.length}`);
const hhFull = rowsFor('s("hh*8")');
const hhMasked = rowsFor('s("hh*8").mask("<0@8 1@16>")');
if (!(hhMasked.length > 0 && hhMasked.length < hhFull.length)) throw new Error(`hh mask: ${hhFull.length}/${hhMasked.length}`);
const patternMask = rowsFor('s("bd*2").mask(sequence(0, 1))');
if (!(patternMask.length > 0 && patternMask.length < bdFull.length)) throw new Error(`Pattern mask: ${bdFull.length}/${patternMask.length}`);

runtime.commitExpression('setcps(1); s("hh*8").mask("<0@8 1@16>")');
const oneBlock = onsets(runtime.queryFrames(0, 48000, 48000), 0);
const split = [...onsets(runtime.queryFrames(0, 24000, 48000), 0), ...onsets(runtime.queryFrames(24000, 24000, 48000), 24000)];
if (JSON.stringify(oneBlock) !== JSON.stringify(split)) throw new Error("mask differs across query blocks");

runtime.commitExpression('setcps(1); note("c4*32").rarely(add(note(12)))');
const rareOneBlock = onsets(runtime.queryFrames(0, 48000, 48000), 0);
const rareAgain = onsets(runtime.queryFrames(0, 48000, 48000), 0);
if (JSON.stringify(rareOneBlock) !== JSON.stringify(rareAgain)) throw new Error("rarely is not repeatable");
const rareSplit = [...onsets(runtime.queryFrames(0, 24000, 48000), 0), ...onsets(runtime.queryFrames(24000, 24000, 48000), 24000)];
if (JSON.stringify(rareOneBlock) !== JSON.stringify(rareSplit)) throw new Error("rarely differs across query blocks");
const notes = rareOneBlock.map((row) => row[3]);
if (!notes.includes(60) || !notes.includes(72)) throw new Error(`rarely did not retain and vary events: ${notes}`);

console.log("strudel_core temporal semantics smoke passed");
