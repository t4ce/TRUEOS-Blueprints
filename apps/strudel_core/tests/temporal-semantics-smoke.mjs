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

function splitRowsFor(source, blockFrames = 2400, totalFrames = 48000) {
  runtime.commitExpression(`setcps(1); ${source}`);
  const rows = [];
  for (let start = 0; start < totalFrames; start += blockFrames) {
    const length = Math.min(blockFrames, totalFrames - start);
    rows.push(...runtime.queryFrames(start, length, 48000).map((row) => [start, row]));
  }
  return rows;
}

// Exact live programs which previously queued silent PCM. An omitted LPF is
// encoded as the native bypass cutoff, and 50ms render blocks preserve their
// mask and note-sequence onsets.
const maskedSaw = splitRowsFor('note("c4*8").s("sawtooth").mask("<1 0 1 0>").gain(.2)', 2400, 4 * 48000);
if (!maskedSaw.length || maskedSaw.some(([, row]) => row[14] !== 24000)) {
  throw new Error(`masked saw did not use native LPF bypass: ${JSON.stringify(maskedSaw)}`);
}
const maskedSawOnsets = maskedSaw
  .filter(([, row]) => row[2] === 0)
  .map(([start, row]) => start + row[0]);
const expectedMaskedSawOnsets = [
  0, 6000, 12000, 18000, 24000, 30000, 36000, 42000,
  96000, 102000, 108000, 114000, 120000, 126000, 132000, 138000,
];
if (JSON.stringify(maskedSawOnsets) !== JSON.stringify(expectedMaskedSawOnsets)) {
  throw new Error(`angle-bracket mask lost upstream slow alternation: ${JSON.stringify(maskedSawOnsets)}`);
}
const sequencedSaw = splitRowsFor('note("c3 e3 g3 c4").s("sawtooth").gain(.25)');
const sequencedNotes = sequencedSaw.filter(([, row]) => row[2] === 0).map(([, row]) => row[8]);
if (JSON.stringify(sequencedNotes) !== JSON.stringify([48, 52, 55, 60])) {
  throw new Error(`50ms blocks lost note sequence: ${JSON.stringify(sequencedNotes)}`);
}
if (sequencedSaw.some(([, row]) => row[14] !== 24000)) {
  throw new Error(`sequenced saw did not use native LPF bypass: ${JSON.stringify(sequencedSaw)}`);
}

// The two masks used by the live shorthand program suppress a proper subset.
const bdFull = rowsFor('s("bd*2")', 0, 20 * 48000);
const bdMasked = rowsFor('s("bd*2").mask("<0@4 1@16>")', 0, 20 * 48000);
if (!(bdMasked.length > 0 && bdMasked.length < bdFull.length)) throw new Error(`bd mask: ${bdFull.length}/${bdMasked.length}`);
const hhFull = rowsFor('s("hh*8")', 0, 24 * 48000);
const hhMasked = rowsFor('s("hh*8").mask("<0@8 1@16>")', 0, 24 * 48000);
if (!(hhMasked.length > 0 && hhMasked.length < hhFull.length)) throw new Error(`hh mask: ${hhFull.length}/${hhMasked.length}`);
const patternMask = rowsFor('s("bd*2").mask(sequence(0, 1))');
if (!(patternMask.length > 0 && patternMask.length < bdFull.length)) throw new Error(`Pattern mask: ${bdFull.length}/${patternMask.length}`);

runtime.commitExpression('setcps(1); s("hh*8").mask("<0@8 1@16>")');
const maskSpanFrames = 24 * 48000;
const oneBlock = onsets(runtime.queryFrames(0, maskSpanFrames, 48000), 0);
const split = [];
for (let start = 0; start < maskSpanFrames; start += 24000) {
  split.push(...onsets(runtime.queryFrames(start, 24000, 48000), start));
}
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
