import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const testDir = dirname(fileURLToPath(import.meta.url));
const jsDir = join(testDir, "..", "js");
for (const file of ["00_fallback_core.js", "10_trueos_adapter.js"]) {
  vm.runInThisContext(await readFile(join(jsDir, file), "utf8"), { filename: file });
}

const bridge = globalThis.__TRUEOS_STRUDEL;
bridge.commitExpression("silence");
bridge.applyInputs([[1, 7, 60, 113, 1, 0]]);
let rows = bridge.queryFrames(0, 2400, 48000);
if (!rows.some((row) => row[4] === 60 && row[5] === 113)) throw new Error("MIDI note-on was not voiced");
bridge.applyInputs([[1, 7, 60, 0, 0, 2400]]);
if (bridge.queryFrames(2400, 2400, 48000).length !== 0) throw new Error("MIDI note-off was not released");

bridge.applyInputs([[2, 1, 29, 100, 1, 4800]]); // HID Z -> C3
rows = bridge.queryFrames(4800, 2400, 48000);
if (!rows.some((row) => row[4] === 48)) throw new Error("keyboard chromatic map did not voice C3");
bridge.applyInputs([[2, 1, 29, 0, 0, 7200], [3, 9, 0, 24, 1, 7200], [3, 9, 1, -32, 1, 7200]]);
rows = bridge.queryFrames(7200, 2400, 48000);
if (!rows.some((row) => row[6] === 3 && row[4] > 60 && row[5] > 96)) throw new Error("pointer sweep/gain was not voiced");

console.log("strudel_core performance input smoke passed");
