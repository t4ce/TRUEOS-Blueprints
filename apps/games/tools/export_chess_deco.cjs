#!/usr/bin/env node
// One-time export of the cube-authored Deco chess collection for Games.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const app = path.resolve(__dirname, '..');
const source = path.resolve(app, '../../../Cubes/Cube/AssetShowcase.html');
const html = fs.readFileSync(source, 'utf8');
const match = html.match(/<script id="cube-chess-core">([\s\S]*?)<\/script>/);
if (!match) throw new Error('cube-chess-core was not found in AssetShowcase.html');
const sandbox = { window: {} };
vm.runInNewContext(match[1], sandbox, { filename: source });
const core = sandbox.window.CUBE_CHESS_CORE;
const roles = [
  ['pawn', 'pawn'],
  ['knight', 'horse'],
  ['bishop', 'knight'], // The rubric has Horse and armored Knight, but no Bishop.
  ['rook', 'rook'],
  ['queen', 'queen'],
  ['king', 'king'],
];
const output = path.join(app, 'assets');
fs.mkdirSync(output, { recursive: true });
for (const [role, rubric] of roles) {
  const model = core.build(rubric, 'deco');
  const records = model.records;
  if (records.length === 0 || records.length > 65535) throw new Error(rubric + ': bad record count');
  const bytes = Buffer.alloc(20 + records.length * 8);
  bytes.write('CUBE', 0, 'ascii');
  bytes[4] = 1;
  bytes[5] = 0;
  bytes[6] = 1;
  bytes[7] = 8;
  bytes.writeUInt16LE(records.length, 8);
  bytes[10] = 1;
  bytes[11] = 4;
  bytes.writeFloatLE(0.2, 12);
  bytes.set([200, 200, 200, 255], 16);
  records.forEach((cell, i) => {
    const o = 20 + i * 8;
    for (const [a, value] of [cell.gx, cell.gy, cell.gz].entries()) {
      if (!Number.isInteger(value) || value < -128 || value > 127) throw new Error(rubric + ': coordinate overflow');
      bytes.writeInt8(value, o + a);
    }
    bytes[o + 3] = 1;
  });
  const file = path.join(output, 'chess_' + role + '.cubes');
  fs.writeFileSync(file, bytes);
  console.log(role + ' <- ' + rubric + '/deco: ' + records.length + ' cubes -> ' + file);
}
