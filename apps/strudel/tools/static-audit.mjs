import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const toolDir = dirname(fileURLToPath(import.meta.url));
const root = join(toolDir, '..');
const read = (path) => readFile(join(root, path), 'utf8');

const tables = await read('apps/strudel_core/src/tables.rs');
const midiBody = tables.match(/MIDI_PHASE_INC_Q32: \[u32; 128\] = \[([\s\S]*?)\];/)?.[1];
const sineBody = tables.match(/SINE_Q15: \[i16; 256\] = \[([\s\S]*?)\];/)?.[1];
if (!midiBody || !sineBody) throw new Error('generated lookup tables not found');
const midi = [...midiBody.matchAll(/(\d+)u32/g)].map((match) => Number(match[1]));
const sine = [...sineBody.matchAll(/(-?\d+)i16/g)].map((match) => Number(match[1]));
if (midi.length !== 128) throw new Error(`MIDI table has ${midi.length} entries`);
if (sine.length !== 256) throw new Error(`sine table has ${sine.length} entries`);
for (let note = 0; note < 128; note += 1) {
  const expected = Math.round((440 * 2 ** ((note - 69) / 12) * 2 ** 32) / 48_000);
  if (midi[note] !== expected) {
    throw new Error(`MIDI phase increment mismatch at ${note}: ${midi[note]} != ${expected}`);
  }
}
for (let index = 0; index < 256; index += 1) {
  const expected = Math.round(Math.sin((2 * Math.PI * index) / 256) * 32_767);
  if (sine[index] !== expected) {
    throw new Error(`sine table mismatch at ${index}: ${sine[index]} != ${expected}`);
  }
}

const main = await read('apps/strudel_core/src/main.rs');
const audio = await read('apps/strudel_core/src/audio_output.rs');
const vm = await read('apps/strudel_core/src/strudel_vm.rs');
for (const [source, needle] of [
  [main, 'render_block(BLOCK_FRAMES, &events)'],
  [audio, 'PlaybackParams::s16le_stereo_48k()'],
  [audio, 'write_interleaved_i16'],
  [vm, 'Workbench::new()'],
  [vm, 'EvalMode::Script'],
  [vm, 'parse_event_rows(&text)'],
]) {
  if (!source.includes(needle)) throw new Error(`integration marker missing: ${needle}`);
}

console.log(JSON.stringify({ midiEntries: midi.length, sineEntries: sine.length, markers: 6 }));
