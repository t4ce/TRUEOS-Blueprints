#!/usr/bin/env node
/** Rebuild the checked-in no-module QuickJS bundle from the pinned npm lock. */
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const app = dirname(dirname(fileURLToPath(import.meta.url)));
const source = join(app, 'tools', 'strudel-upstream');
const output = join(app, 'js', 'vendor', 'strudel-core.bundle.js');
const work = await mkdtemp(join(tmpdir(), 'trueos-strudel-'));
try {
  await cp(source, work, { recursive: true });
  const npm = spawnSync('npm', ['ci', '--ignore-scripts', '--no-audit', '--no-fund'], { cwd: work, stdio: 'inherit' });
  if (npm.status !== 0) throw new Error('npm ci failed');
  const coreAlias = join(work, 'core-alias');
  await cp(join(work, 'node_modules', '@strudel', 'core'), coreAlias, { recursive: true });
  await cp(join(work, 'core-shim.mjs'), join(coreAlias, 'index.mjs'));
  await writeFile(join(coreAlias, 'package.json'), '{"type":"module","main":"index.mjs"}\n');
  const esbuild = join(work, 'node_modules', '.bin', 'esbuild');
  const build = spawnSync(esbuild, [join(work, 'entry.mjs'), '--bundle', '--format=iife', '--platform=browser', '--target=es2020', '--minify', `--alias:@strudel/core=${coreAlias}`, `--outfile=${output}`], { stdio: 'inherit' });
  if (build.status !== 0) throw new Error('esbuild failed');
  const bundle = await readFile(output);
  if (/AudioContext|webkitAudioContext|WebAudio/.test(bundle)) throw new Error('refusing to vendor browser audio code');
  console.log(`wrote ${output} (${bundle.length} bytes, sha256=${createHash('sha256').update(bundle).digest('hex')})`);
} finally {
  await rm(work, { recursive: true, force: true });
}
