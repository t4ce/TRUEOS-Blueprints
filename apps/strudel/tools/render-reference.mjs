#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const SAMPLE_RATE = 48_000;
const BLOCK_FRAMES = 2_400;
const TOTAL_FRAMES = 96_000; // two seconds / one cycle at 0.5 cps
const ATTACK_FRAMES = 240;
const RELEASE_FRAMES = 960;
const Q15_ONE = 32_767;
const MIX_HEADROOM_DIVISOR = 3;

const toolDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(toolDir, '..');
const appJs = join(repoRoot, 'apps', 'strudel_core', 'js');
const output = resolve(process.argv[2] || join(toolDir, '..', 'tests', 'reference-demo-2s.wav'));
const context = vm.createContext({ console, globalThis: null });
context.globalThis = context;

for (const file of ['00_fallback_core.js', '10_trueos_adapter.js', '20_demo_pattern.js']) {
  vm.runInContext(await readFile(join(appJs, file), 'utf8'), context, { filename: file });
}

const midiPhase = Array.from({ length: 128 }, (_, note) =>
  Math.round(((440 * 2 ** ((note - 69) / 12)) / SAMPLE_RATE) * 2 ** 32),
);
const sine = Array.from({ length: 256 }, (_, index) =>
  Math.round(Math.sin((2 * Math.PI * index) / 256) * Q15_ONE),
);
const pcm = new Int16Array(TOTAL_FRAMES * 2);

for (let absolute = 0; absolute < TOTAL_FRAMES; absolute += BLOCK_FRAMES) {
  const rows = vm.runInContext(
    `globalThis.__TRUEOS_STRUDEL.queryFrames(${absolute},${BLOCK_FRAMES},${SAMPLE_RATE},1,2)`,
    context,
  );
  const block = renderBlock(BLOCK_FRAMES, rows);
  pcm.set(block, absolute * 2);
}

const wav = encodeWav(pcm, SAMPLE_RATE, 2);
await writeFile(output, wav);

let peak = 0;
let energy = 0;
let nonZero = 0;
for (const sample of pcm) {
  const magnitude = Math.abs(sample);
  peak = Math.max(peak, magnitude);
  energy += sample * sample;
  if (sample !== 0) nonZero += 1;
}
const relativeOutput = relative(repoRoot, output);
const report = {
  output: relativeOutput.startsWith('..') ? output : relativeOutput,
  sampleRate: SAMPLE_RATE,
  channels: 2,
  frames: TOTAL_FRAMES,
  seconds: TOTAL_FRAMES / SAMPLE_RATE,
  peak,
  rms: Math.sqrt(energy / pcm.length),
  nonZeroSamples: nonZero,
  sha256: createHash('sha256').update(wav).digest('hex'),
};
console.log(JSON.stringify(report, null, 2));

function renderBlock(frameCount, rows) {
  const mix = new Int32Array(frameCount * 2);

  for (const row of rows) {
    const [rawStart, rawEnd, rawAge, duration, note, velocity, waveform, panQ15] = row;
    const start = Math.max(0, Math.min(frameCount, rawStart));
    const end = Math.max(0, Math.min(frameCount, rawEnd));
    if (start >= end || velocity === 0) continue;

    const increment = midiPhase[note];
    let phase = moduloU32(increment * rawAge);
    const leftGain = panQ15 > 0 ? Q15_ONE - panQ15 : Q15_ONE;
    const rightGain = panQ15 < 0 ? Q15_ONE + panQ15 : Q15_ONE;

    for (let frame = start; frame < end; frame += 1) {
      const age = rawAge + (frame - start);
      const envelope = envelopeQ15(age, duration);
      if (envelope !== 0) {
        const raw = waveSample(waveform, phase, age);
        const voiced = Math.trunc(
          (raw * velocity * envelope) / (127 * Q15_ONE * MIX_HEADROOM_DIVISOR),
        );
        const left = Math.trunc((voiced * leftGain) / Q15_ONE);
        const right = Math.trunc((voiced * rightGain) / Q15_ONE);
        mix[frame * 2] = saturatingI32(mix[frame * 2] + left);
        mix[frame * 2 + 1] = saturatingI32(mix[frame * 2 + 1] + right);
      }
      phase = moduloU32(phase + increment);
    }
  }

  const output = new Int16Array(frameCount * 2);
  for (let index = 0; index < mix.length; index += 1) {
    output[index] = Math.max(-32_768, Math.min(32_767, mix[index]));
  }
  return output;
}

function envelopeQ15(age, duration) {
  if (age >= duration) return 0;
  const attack = age >= ATTACK_FRAMES ? Q15_ONE : Math.floor((age * Q15_ONE) / ATTACK_FRAMES);
  const remaining = duration - age;
  const release =
    remaining >= RELEASE_FRAMES
      ? Q15_ONE
      : Math.floor((remaining * Q15_ONE) / RELEASE_FRAMES);
  return Math.min(attack, release);
}

function waveSample(waveform, phase, age) {
  switch (waveform) {
    case 1:
      return (phase & 0x8000_0000) === 0 ? 32_767 : -32_767;
    case 2:
      return (phase >>> 16) - 32_768;
    case 3: {
      const x = phase >>> 16;
      const value = x < 32_768 ? x * 2 - 32_768 : 98_302 - x * 2;
      return Math.max(-32_768, Math.min(32_767, value));
    }
    case 4: {
      let x = (phase ^ Math.imul(age >>> 0, 0x9e37_79b9)) >>> 0;
      x = (x ^ (x << 13)) >>> 0;
      x = (x ^ (x >>> 17)) >>> 0;
      x = (x ^ (x << 5)) >>> 0;
      const high = x >>> 16;
      return high >= 32_768 ? high - 65_536 : high;
    }
    default:
      return sine[phase >>> 24];
  }
}

function moduloU32(value) {
  return value - Math.floor(value / 2 ** 32) * 2 ** 32;
}

function saturatingI32(value) {
  return Math.max(-2_147_483_648, Math.min(2_147_483_647, value));
}

function encodeWav(samples, sampleRate, channels) {
  const dataBytes = samples.length * 2;
  const buffer = Buffer.alloc(44 + dataBytes);
  buffer.write('RIFF', 0, 'ascii');
  buffer.writeUInt32LE(36 + dataBytes, 4);
  buffer.write('WAVE', 8, 'ascii');
  buffer.write('fmt ', 12, 'ascii');
  buffer.writeUInt32LE(16, 16);
  buffer.writeUInt16LE(1, 20);
  buffer.writeUInt16LE(channels, 22);
  buffer.writeUInt32LE(sampleRate, 24);
  buffer.writeUInt32LE(sampleRate * channels * 2, 28);
  buffer.writeUInt16LE(channels * 2, 32);
  buffer.writeUInt16LE(16, 34);
  buffer.write('data', 36, 'ascii');
  buffer.writeUInt32LE(dataBytes, 40);
  for (let index = 0; index < samples.length; index += 1) {
    buffer.writeInt16LE(samples[index], 44 + index * 2);
  }
  return buffer;
}
