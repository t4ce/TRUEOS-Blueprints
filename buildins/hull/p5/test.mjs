import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

// Run with no Node APIs available to the parser itself.
const context = vm.createContext({});
vm.runInContext(fs.readFileSync(new URL('./parse5.js', import.meta.url), 'utf8'), context);
const { parse, parseFragment, serialize } = context.parse5;
assert.equal(serialize(parse('<title>Test</title><p>A &amp; B &copy;')), '<html><head><title>Test</title></head><body><p>A &amp; B ©</p></body></html>');
assert.equal(serialize(parseFragment('<table>outside<tr><td>cell</table>')), 'outside<table><tbody><tr><td>cell</td></tr></tbody></table>');
assert.equal(serialize(parseFragment('<b><i>x</b>y</i>')), '<b><i>x</i></b><i>y</i>');
assert.equal(serialize(parseFragment('<svg><circle /></svg>')), '<svg><circle></circle></svg>');
assert.equal(serialize(parseFragment('<script>if (a < b) x="&amp;";</script>')), '<script>if (a < b) x="&amp;";</script>');
console.log('parse5 bundle: document, entities, table recovery, formatting recovery, SVG and raw-text checks passed');
