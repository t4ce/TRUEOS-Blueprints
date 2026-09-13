#!/usr/bin/env node
// Export the current default WorldShowcase generation as the runtime's compact
// CUBES v2 assets: c1 coordinates, packed c4 terrain, exact c2 portal frames,
// and two metadata-only arrival markers per portal.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const {createHash} = require('crypto');

const PORTAL_MARKER_FACES = ['north', 'east', 'south', 'west', 'bottom', 'top', 'center'];
const PORTAL_MARKER_FIRST = 17;
const PORTAL_MARKER_LAST = 30;

const html = fs.readFileSync(path.join(__dirname, 'WorldShowcase.html'), 'utf8');
function between(start, end) {
  const a = html.indexOf(start);
  const b = html.indexOf(end, a);
  if (a < 0 || b < 0) throw new Error(`cannot extract generator section: ${start}`);
  return html.slice(a, b);
}

// The showcase deliberately keeps its deterministic world model independent
// of its WebGL/UI layer. Evaluate only that model, then generate all defaults.
const model = [
  // Geometry math and deterministic color/seed helpers only. No camera,
  // renderer, controls, or editor state enters this export model.
  between('const clamp=', 'function lookQuaternion'),
  between('function hexRGB', 'function cubeGeometryFromGLB'),
  between('const WORLD_THEMES', 'function geometryOverlapsBox'),
  `
  for (const world of WORLDS) generatePlatforms(world, levelData[world.id - 1]);
  globalThis.__api = { platformHulls, balancedPortalCells, V3, themeIndexAt, hashSeed, PALETTES, RAINBOW, themeFor };
  globalThis.__lvl27 = WORLDS.map((world) => ({
    world,
    data: levelData[world.id - 1],
    geometry: buildGeometry(world, levelData[world.id - 1]),
    portals: portalInfo(world, levelData[world.id - 1]),
  }));
  `,
].join('\n');
const MATERIAL_PALETTE = require('./material-palette.js').parse(
  JSON.parse(fs.readFileSync(path.join(__dirname, 'subcubes-materials.json'), 'utf8')));
const context = { console, globalThis: {}, MATERIAL_PALETTE };
vm.createContext(context);
vm.runInContext(model, context, { filename: 'WorldShowcase-default-model.js' });

function rgba(hex) {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255, 255];
}
function colorFor(entry, voxel) {
  const api = context.globalThis.__api;
  const { world, data } = entry;
  if (world.kind === 'special') {
    // Strict-grid assets cap at 16,384 records. The studio's one-c4-cell
    // rainbow would exceed that limit, so retain its palette as deterministic
    // 4×4×4 c4 colour fields, which can be represented as c4 records.
    const macro = [voxel.x, voxel.y, voxel.z].map(v => Math.floor((v + 1024) / 32));
    return api.RAINBOW[api.hashSeed(macro.join(',')) % api.RAINBOW.length];
  }
  const theme = voxel.theme ?? world.themes[api.themeIndexAt(
    world,
    new api.V3(voxel.x + 4, voxel.y + 4, voxel.z + 4),
    api.hashSeed(data.config.seed),
  )];
  return api.PALETTES[theme];
}

function compact(entry) {
  const occupied = new Map();
  for (const voxel of entry.geometry.primary.values()) {
    // Defaults have no c3 growth and no placed c1 controls. All source voxels
    // are full c4 cells, which are exactly representable by this export grid.
    if (voxel.size !== 8) throw new Error(`unexpected non-c4 voxel in world ${entry.world.id}`);
    const cell = [(voxel.x + 1024) / 8, (voxel.y + 1024) / 8, (voxel.z + 1024) / 8];
    if (!cell.every(Number.isInteger) || !cell.every(n => n >= 0 && n < 256)) throw new Error(`out-of-range c4 cell in world ${entry.world.id}`);
    // Parts 9/10 retain connector/path ownership; 11/12 identify portal frames.
    // Generated paths can occupy a run before portalRunVoxels visits it.
    // Classify by the actual connector volume, preserving all source geometry.
    const portal = entry.portals.find(o => ['x','y','z'].every(a => voxel[a] >= o.runLo[a] && voxel[a]+8 <= o.runHi[a]));
    const part = portal ? (portal.face === 'center' ? 13 : ((cell[0] + cell[1] + cell[2]) & 1 ? 9 : 10)) : 0;
    occupied.set(cell.join(','), { cell, color: colorFor(entry, voxel), part });
  }
  const used = new Set();
  const records = [];
  const cells = [...occupied.values()].sort((a, b) => a.cell[0] - b.cell[0] || a.cell[1] - b.cell[1] || a.cell[2] - b.cell[2]);
  const key = (x, y, z) => `${x},${y},${z}`;
  for (const item of cells) {
    const [x, y, z] = item.cell;
    if (used.has(key(x, y, z))) continue;
    let tier = 1;
    for (const candidate of [4, 3, 2]) {
      let matches = true;
      for (let dx = 0; dx < candidate && matches; dx++) for (let dy = 0; dy < candidate && matches; dy++) for (let dz = 0; dz < candidate; dz++) {
        const other = occupied.get(key(x + dx, y + dy, z + dz));
        if (!other || used.has(key(x + dx, y + dy, z + dz)) || other.color !== item.color || other.part !== item.part) { matches = false; break; }
      }
      if (matches) { tier = candidate; break; }
    }
    for (let dx = 0; dx < tier; dx++) for (let dy = 0; dy < tier; dy++) for (let dz = 0; dz < tier; dz++) used.add(key(x + dx, y + dy, z + dz));
    records.push({ x: (x - 128)*8, y: (y - 128)*8, z: (z - 128)*8, tier: tier*8, sourceTier: 8, color: item.color, part: item.part });
  }
  if (used.size !== occupied.size) throw new Error(`incomplete compaction in world ${entry.world.id}`);
  const api = context.globalThis.__api;
  const markers = [];
  for (const portal of entry.portals) {
    const painted = api.balancedPortalCells(portal.shape, portal.mix, portal.settings.rotation);
    for (const cell of painted.cells) {
      const p = new api.V3(cell.u, cell.v, cell.z).applyQuaternion(portal.q).add(portal.pos);
      const xyz = [p.x-1,p.y-1,p.z-1].map(Math.round);
      if ([p.x-1,p.y-1,p.z-1].some((v,i)=>Math.abs(v-xyz[i])>1e-6)) throw new Error('off-grid portal');
      let color = painted.palette[cell.colour].hex;
      if (portal.edge.action === 'leave') {
        const rgb = rgba(color).slice(0,3).map((v,i)=>Math.round(v*.92+[.97,.98,.95][i]*255*.08));
        color = '#'+rgb.map(v=>v.toString(16).padStart(2,'0')).join('');
      }
      records.push({x:xyz[0],y:xyz[1],z:xyz[2],tier:2,sourceTier:2,color,part:portal.face === 'center' ? 15 : 11});
    }
    const face = PORTAL_MARKER_FACES.indexOf(portal.face);
    if (face < 0) throw new Error(`unknown portal marker face ${portal.face}`);
    const spawn = portal.spawnMarker.toArray(), forward = portal.forwardMarker.toArray();
    const spawnPoint = portal.spawnPoint.toArray(), support = portal.spawnSupport.toArray();
    if (!spawn.every((v, axis) => Math.abs(v - spawnPoint[axis] - support[axis]) < 1e-6)) {
      throw new Error(`invalid portal marker support in world ${entry.world.id} ${portal.face}`);
    }
    const delta = forward.map((v, axis) => v - spawn[axis]);
    if (!delta.every((v, axis) => Math.abs(v - portal.normal.toArray()[axis] * 8) < 1e-6)) {
      throw new Error(`invalid portal marker direction in world ${entry.world.id} ${portal.face}`);
    }
    for (const [center, part] of [[spawn, PORTAL_MARKER_FIRST + face * 2], [forward, PORTAL_MARKER_FIRST + face * 2 + 1]]) {
      const xyz = center.map(v => Math.round(v - 1));
      if (center.some((v, axis) => Math.abs(v - 1 - xyz[axis]) > 1e-6)) throw new Error('off-grid portal marker');
      markers.push({x:xyz[0],y:xyz[1],z:xyz[2],tier:2,sourceTier:2,part});
    }
  }
  // Marker records are appended so filtering them leaves every existing render
  // cube ID stable. Their palette entry is irrelevant at runtime; reusing an
  // existing terrain colour avoids adding a display-only material.
  if (!records.length) throw new Error(`world ${entry.world.id} has no render records`);
  for (const marker of markers) records.push({...marker,color:records[0].color});
  return records;
}

function encode(entry) {
  const records = compact(entry);
  if (records.length > 16384) throw new Error(`world ${entry.world.id} has ${records.length} records (runtime limit is 16384)`);
  const palette = [];
  const paletteIndex = new Map();
  for (const record of records) if (!paletteIndex.has(record.color)) {
    paletteIndex.set(record.color, palette.length);
    palette.push(rgba(record.color));
  }
  const output = Buffer.alloc(16 + palette.length * 4 + records.length * 12);
  output.write('CUBE', 0, 'ascii'); output[4] = 2; output[5] = 0; output[6] = 14; output[7] = 12;
  output.writeUInt16LE(records.length, 8); output[10] = palette.length; output[11] = 4; output.writeFloatLE(0.2, 12);
  let offset = 16;
  for (const color of palette) { Buffer.from(color).copy(output, offset); offset += 4; }
  for (const record of records) {
    output.writeInt16LE(record.x, offset); output.writeInt16LE(record.y, offset + 2); output.writeInt16LE(record.z, offset + 4);
    output[offset + 6] = record.tier; output[offset + 7] = paletteIndex.get(record.color); output[offset + 8] = record.part; output[offset + 9] = record.sourceTier;
    offset += 12;
  }
  // Map decoded (constituent) cube IDs, not packed record IDs. Compaction can
  // cross an ownership boundary; recovering each cell preserves that boundary.
  const hulls = context.globalThis.__api.platformHulls(entry.geometry).map(h=>({...h,ranges:[],rgb:[0,0,0],count:0}));
  const byOwner = new Map(hulls.map(h=>[h.owner,h]));
  let id=0;
  for(const r of records){
    // Metadata markers are not decoded into render/collision cubes and do not
    // consume IDs in the platform-ownership sidecar.
    if(r.part>=PORTAL_MARKER_FIRST&&r.part<=PORTAL_MARKER_LAST)continue;
    for(let x=0;x<r.tier;x+=r.sourceTier)for(let y=0;y<r.tier;y+=r.sourceTier)for(let z=0;z<r.tier;z+=r.sourceTier,id++){
      const v=entry.geometry.primary.get([r.x+x,r.y+y,r.z+z].join(','));
      const h=r.part===0&&r.sourceTier===8?byOwner.get(v?.owner):null;
      if(!h)continue;
      const last=h.ranges[h.ranges.length-1];
      if(last&&last[1]===id)last[1]++;else h.ranges.push([id,id+1]);
      rgba(r.color).slice(0,3).forEach((c,a)=>h.rgb[a]+=c);h.count++;
    }
  }
  for(const h of hulls){if(!h.count)throw new Error('empty platform hull');h.rgb=h.rgb.map(c=>Math.round(c/h.count));}
  return { output, hulls, decoded:id, records: records.length, palette: palette.length, source: entry.geometry.primary.size };
}

const outputDir = path.join(__dirname, 'lvl27');
const outputs = context.globalThis.__lvl27.map(entry => {
  const encoded = encode(entry);
  const slug = entry.world.kind === 'special'
    ? 'void'
    : entry.world.themes.map(id => context.globalThis.__api.themeFor(id).name.toLowerCase()).join('_');
  const filename = `world_${String(entry.world.id).padStart(2, '0')}_${slug}.cubes`;
  return {filename, encoded};
});
for (const {filename, encoded} of outputs) {
  if (process.argv.includes('--check')) {
    if (!fs.readFileSync(path.join(outputDir, filename)).equals(encoded.output)) throw new Error(`stale export: ${filename}`);
  } else fs.writeFileSync(path.join(outputDir, filename), encoded.output);
  console.log(`${filename}: ${encoded.source} c4 cells -> ${encoded.records} records, ${encoded.palette} colours`);
}

const manifest=Buffer.from(JSON.stringify({version:1,worlds:outputs.map(({filename,encoded})=>({
  filename,sha256:createHash('sha256').update(encoded.output).digest('hex'),decoded:encoded.decoded,hulls:encoded.hulls,
}))},null,2)+'\n');
const manifestPath=path.join(outputDir,'platform-hulls.json');
if(process.argv.includes('--check')){
  if(!fs.readFileSync(manifestPath).equals(manifest))throw new Error('stale platform hull export');
}else fs.writeFileSync(manifestPath,manifest);
console.log('platform-hulls.json: ownership and padded hulls for all 27 worlds');
