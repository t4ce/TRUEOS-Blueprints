#!/usr/bin/env node
// Generate canonical parse5 trees, or compare them with rp5's native oracle.
// No npm install is needed: the checked-in ../p5/parse5.js is the oracle.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const testsDirectory = path.dirname(fileURLToPath(import.meta.url));
const crateDirectory = path.dirname(testsDirectory);
const namespaceUris = {
  html: 'http://www.w3.org/1999/xhtml',
  svg: 'http://www.w3.org/2000/svg',
  mathml: 'http://www.w3.org/1998/Math/MathML',
};

let fixturesPath = path.join(testsDirectory, 'fixtures.json');
let nativeExecutable;
let filter = '';
let check = false;
let writePath;
let reportPath;
for (let index = 2; index < process.argv.length; index++) {
  const arg = process.argv[index];
  if (arg === '--check') check = true;
  else if (arg === '--fixtures') fixturesPath = path.resolve(process.argv[++index]);
  else if (arg === '--native') nativeExecutable = path.resolve(process.argv[++index]);
  else if (arg === '--filter') filter = process.argv[++index];
  else if (arg === '--write') writePath = path.resolve(process.argv[++index]);
  else if (arg === '--report') { reportPath = path.resolve(process.argv[++index]); check = true; }
  else if (arg === '--help') {
    console.log('Usage: node tests/parse5_oracle.mjs [--check] [--native EXE] [--fixtures JSON] [--filter NAME] [--write JSON] [--report JSON]');
    console.log('Default: print canonical parse5 trees. --check: compare with cargo run --example oracle -- fixtures.json.');
    process.exit(0);
  } else throw new Error(`Unknown option: ${arg}`);
}

const allFixtures = JSON.parse(fs.readFileSync(fixturesPath, 'utf8'));
const fixtures = allFixtures.filter((fixture) => fixture.name.includes(filter));
assert(fixtures.length > 0, `No fixtures match ${JSON.stringify(filter)}`);
assert.equal(new Set(allFixtures.map((fixture) => fixture.name)).size, allFixtures.length, 'Fixture names must be unique');

// The bundled parser runs without Node APIs in its own realm.
const realm = vm.createContext({});
vm.runInContext(fs.readFileSync(path.join(crateDirectory, '../p5/parse5.js'), 'utf8'), realm);
const { parse, parseFragment, defaultTreeAdapter: adapter } = realm.parse5;

function canonical(node) {
  switch (node.nodeName) {
    case '#document':
      return { type: 'document', mode: node.mode, children: node.childNodes.map(canonical) };
    case '#document-fragment':
      return { type: 'fragment', children: node.childNodes.map(canonical) };
    case '#text':
      return { type: 'text', value: node.value };
    case '#comment':
      return { type: 'comment', value: node.data };
    case '#documentType':
      return { type: 'doctype', name: node.name, public_id: node.publicId, system_id: node.systemId };
    default: {
      assert.equal(typeof node.tagName, 'string', `Unexpected node: ${node.nodeName}`);
      const attributes = Array.from(node.attrs, (attr) => ({
        name: attr.name,
        namespace: attr.namespace ?? '',
        prefix: attr.prefix ?? '',
        value: attr.value,
      })).sort((a, b) => {
        const left = [a.namespace, a.name, a.prefix];
        const right = [b.namespace, b.name, b.prefix];
        for (let i = 0; i < left.length; i++) {
          if (left[i] < right[i]) return -1;
          if (left[i] > right[i]) return 1;
        }
        return 0;
      });
      const result = {
        type: 'element',
        name: node.tagName,
        namespace: node.namespaceURI,
        attributes,
        children: Array.from(node.childNodes, canonical),
      };
      if (node.content) result.template_content = canonical(node.content);
      return result;
    }
  }
}

function parseFixture(fixture) {
  assert.equal(typeof fixture.html, 'string', `Missing HTML in ${fixture.name}`);
  if (fixture.mode === 'document') {
    assert(!fixture.context, `Document fixture ${fixture.name} cannot have a fragment context`);
    return { name: fixture.name, tree: canonical(parse(fixture.html)) };
  }
  assert.equal(fixture.mode, 'fragment', `Invalid mode in ${fixture.name}`);
  let context = null;
  if (fixture.context) {
    const namespace = namespaceUris[fixture.context.namespace];
    assert(namespace, `Invalid namespace in ${fixture.name}`);
    context = adapter.createElement(fixture.context.tag, namespace, []);
    if (fixture.context.tag === 'template' && fixture.context.namespace === 'html') {
      adapter.setTemplateContent(context, adapter.createDocumentFragment());
    }
  }
  return { name: fixture.name, tree: canonical(parseFragment(context, fixture.html)) };
}

// A JSON round-trip makes oracle objects ordinary host-realm objects.
const expected = JSON.parse(JSON.stringify(fixtures.map(parseFixture)));
if (writePath) fs.writeFileSync(writePath, `${JSON.stringify(expected, null, 2)}\n`);
if (!check) {
  if (!writePath) process.stdout.write(`${JSON.stringify(expected, null, 2)}\n`);
  process.exit(0);
}

const command = nativeExecutable ?? 'cargo';
const args = nativeExecutable
  ? [fixturesPath]
  : ['run', '--quiet', '--manifest-path', path.join(crateDirectory, 'Cargo.toml'), '--example', 'oracle', '--', fixturesPath];
// Running Cargo outside the Blueprint workspace avoids its build-std settings:
// the oracle is an ordinary host executable, while rp5 itself stays no_std.
const run = spawnSync(command, args, { cwd: os.tmpdir(), encoding: 'utf8', timeout: 60000, maxBuffer: 16 * 1024 * 1024 });
if (run.error) throw run.error;
if (run.status !== 0) {
  process.stderr.write(run.stderr ?? '');
  throw new Error(`Native oracle exited with ${run.status ?? run.signal}`);
}
const actual = JSON.parse(run.stdout);
assert(Array.isArray(actual), 'Native oracle output must be an array of {name,tree}');
const byName = new Map(actual.map((result) => [result.name, result]));
assert.equal(byName.size, actual.length, 'Native oracle result names must be unique');

function firstDifference(left, right, at = 'tree') {
  if (Object.is(left, right)) return null;
  if (typeof left !== typeof right || left === null || right === null || typeof left !== 'object') {
    return { at, expected: left, actual: right };
  }
  if (Array.isArray(left) !== Array.isArray(right)) return { at, expected: left, actual: right };
  if (Array.isArray(left) && left.length !== right.length) {
    return { at: `${at}.length`, expected: left.length, actual: right.length };
  }
  for (const key of new Set([...Object.keys(left), ...Object.keys(right)])) {
    const diff = firstDifference(left[key], right[key], Array.isArray(left) ? `${at}[${key}]` : `${at}.${key}`);
    if (diff) return diff;
  }
  return null;
}

function fixtureGroup(name) {
  return name.match(/^(html5lib_tree_.+)_L\d+$/)?.[1]
    ?? name.match(/^(html5lib_token_.+)_\d+_(?:data|rcdata|rawtext|script_data|plaintext)$/)?.[1]
    ?? name.split('_')[0];
}

function brief(value) {
  const serialized = JSON.stringify(value) ?? 'undefined';
  return serialized.length > 400 ? `${serialized.slice(0, 400)}…` : serialized;
}

const failures = [];
const groups = new Map();
const fixtureByName = new Map(fixtures.map((fixture) => [fixture.name, fixture]));
for (const result of expected) {
  const groupName = fixtureGroup(result.name);
  const group = groups.get(groupName) ?? { name: groupName, total: 0, failed: 0 };
  groups.set(groupName, group);
  group.total++;
  const observed = byName.get(result.name);
  const difference = firstDifference(result.tree, observed?.tree);
  if (difference) {
    group.failed++;
    failures.push({ name: result.name, fixture: fixtureByName.get(result.name), ...difference });
    if (failures.length <= 20) {
      console.error(`FAIL ${result.name}: ${difference.at}`);
      console.error(`  parse5: ${brief(difference.expected)}`);
      console.error(`  rp5:    ${brief(difference.actual)}`);
    }
  }
}
const expectedNames = new Set(expected.map((result) => result.name));
const unexpectedNames = filter ? [] : actual.map((result) => result.name).filter((name) => !expectedNames.has(name));
const groupResults = [...groups.values()].sort((a, b) => b.failed - a.failed || a.name.localeCompare(b.name));
if (reportPath) {
  fs.writeFileSync(reportPath, `${JSON.stringify({ total: fixtures.length, passed: fixtures.length - failures.length,
    failed: failures.length, unexpected_names: unexpectedNames, groups: groupResults, failures }, null, 2)}\n`);
}
if (failures.length || unexpectedNames.length) {
  if (failures.length > 20) console.error(`${failures.length - 20} additional failures; use --report JSON for all differences`);
  for (const group of groupResults.filter((group) => group.failed)) {
    console.error(`  ${group.name}: ${group.failed}/${group.total} failed`);
  }
  if (unexpectedNames.length) console.error(`Unexpected native results: ${unexpectedNames.join(', ')}`);
  console.error(`${failures.length}/${fixtures.length} parse5 parity fixtures failed`);
  process.exitCode = 1;
} else {
  console.log(`rp5 matches parse5 on ${fixtures.length} document and fragment fixtures`);
}
