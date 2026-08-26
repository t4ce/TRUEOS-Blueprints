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

const source = `
setcps(1)
stack(
  note("[<g1 f1>/8](<3 5>,8)")
    .clip(perlin.range(.15,1.5)).release(.1).s("sawtooth")
    .lpf(sine.range(400,800).slow(16)).lpq(cosine.range(6,14).slow(3))
    .lpenv(sine.mul(4).slow(4)).lpd(.2).lpa(.02).ftype('24db')
    .rarely(add(note(12))).room(.2).shape(.3).postgain(.5)
    .superimpose(x=>x.add(note(12)).delay(.5).bpf(1000))
    .gain("[.2 1@3]*2"),
  stack(
    s("bd*2").mask("<0@4 1@16>"),
    s("hh*8").gain(saw.mul(saw.fast(2))).clip(sine).mask("<0@8 1@16>")
  ).bank('RolandTR909')
)
`;

globalThis.__TRUEOS_STRUDEL.commitExpression("setcps(1); s('bd*2').bank('trueos')");
const registered = globalThis.__TRUEOS_STRUDEL.queryFrames(0, 48000, 48000);
if (!registered.some((row) => row[6] === 2)) throw new Error(`trueos bank did not emit sample rows: ${JSON.stringify(registered)}`);

const before = globalThis.__TRUEOS_STRUDEL.status();
const committed = globalThis.__TRUEOS_STRUDEL.commitExpression(source);
if (committed.cpsNumerator !== 1 || committed.cpsDenominator !== 1) {
  throw new Error(`top-level setcps was not committed: ${JSON.stringify(committed)}`);
}
const rows = globalThis.__TRUEOS_STRUDEL.queryFrames(0, 48000, 48000);
if (!Array.isArray(rows) || rows.length < 3) throw new Error("shorthand program emitted too few voices");
let saw = false;
let percussion = false;
for (const row of rows) {
  if (!Array.isArray(row) || row.length !== 30) throw new Error(`not a native v2 row: ${JSON.stringify(row)}`);
  if (row.some((value) => !Number.isInteger(value))) throw new Error(`native row is not integral: ${JSON.stringify(row)}`);
  if (row[0] < 0 || row[1] <= row[0] || row[1] > 48000 || row[8] < 0 || row[8] > 127 || row[9] < 0 || row[9] > 32767) {
    throw new Error(`native row out of bounds: ${JSON.stringify(row)}`);
  }
  saw ||= row[7] === 2;
  percussion ||= row[7] === 4 || row[8] === 36;
}
if (!saw || !percussion) throw new Error(`expected oscillator and synthesized percussion: ${JSON.stringify(rows)}`);
const voiced = rows.find((row) => row[7] === 2);
if (!voiced || voiced[23] !== 240 || voiced[25] !== 4800 || voiced[26] !== 960 || voiced[27] !== 9600 || voiced[28] !== 32767) {
  throw new Error(`V2 envelope controls were not encoded: ${JSON.stringify(voiced)}`);
}

let rejected = false;
try { globalThis.__TRUEOS_STRUDEL.commitExpression("setcps(.125); ({ nope: true })"); } catch (_) { rejected = true; }
if (!rejected) throw new Error("invalid shorthand transaction was accepted");
const after = globalThis.__TRUEOS_STRUDEL.status();
if (after.revision !== committed.revision || after.cpsNumerator !== 1 || before.revision >= after.revision) {
  throw new Error(`failed transaction changed active state: ${JSON.stringify(after)}`);
}

console.log("strudel_core shorthand/native smoke passed");
