import fs from 'node:fs';
import assert from 'node:assert/strict';
import { decodeHTML, decodeHTMLAttribute } from '../../p5/node_modules/entities/dist/index.js';
const refs = JSON.parse(fs.readFileSync(new URL('./entities_reference.json', import.meta.url), 'utf8'));
for (const [name, value] of Object.entries(refs)) {
    assert.equal(decodeHTML('&'+name), value, name);
    if (name.endsWith(';')) assert.equal(decodeHTMLAttribute('&'+name), value, name);
}
console.log(`Verified ${Object.keys(refs).length} named references against vendored entities decoder`);
