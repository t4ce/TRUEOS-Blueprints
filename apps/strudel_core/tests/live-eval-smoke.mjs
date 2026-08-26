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
const demo = await readFile(join(jsDir, "20_demo_pattern.js"), "utf8");
const initial = bridge.commitExpression(demo);
if (initial.source !== "fallback" || initial.revision !== 1) {
  throw new Error(`unexpected initial status: ${JSON.stringify(initial)}`);
}

const canonical = bridge.selfTest();
if (canonical !== "a@0.000000-0.500000|b@0.500000-0.750000|c@0.750000-1.000000") {
  throw new Error(`canonical temporal smoke failed: ${canonical}`);
}

const committed = bridge.commitExpression(`
  stack(
    sequence({ note: "c4", wave: "triangle" }, { note: "g4", pan: 0.25 }),
    sequence({ note: "c2", wave: "square" }, null)
  ).fast(2)
`);
if (committed.revision !== 2) {
  throw new Error(`commit did not advance revision: ${JSON.stringify(committed)}`);
}

const beforeFailure = JSON.stringify(bridge.queryFrames(0, 2400, 48000, 1, 2));
let failed = false;
try {
  bridge.commitExpression('({ not: "a pattern" })');
} catch (error) {
  failed = /expects a Strudel Pattern/.test(String(error));
}
if (!failed) throw new Error("invalid commit did not report a Pattern type error");
const afterFailure = JSON.stringify(bridge.queryFrames(0, 2400, 48000, 1, 2));
if (beforeFailure !== afterFailure) throw new Error("failed commit changed the active pattern");
if (bridge.status().revision !== 2) throw new Error("failed commit changed the revision");

let nestedFailed = false;
try {
  bridge.commitExpression(`
    (__TRUEOS_STRUDEL.commitExpression('sequence("d4")'), { not: "a pattern" })
  `);
} catch (error) {
  nestedFailed = /nested pattern commits are not allowed/.test(String(error));
}
if (!nestedFailed) throw new Error("nested commit was not rejected");
if (JSON.stringify(bridge.queryFrames(0, 2400, 48000, 1, 2)) !== beforeFailure) {
  throw new Error("nested commit changed the active pattern");
}
if (bridge.status().revision !== 2) throw new Error("nested commit changed the revision");

console.log("strudel_core live expression smoke passed");
