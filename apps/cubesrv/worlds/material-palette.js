/* Shared data loader for the showcases and the offline world exporter. */
(function (root) {
  'use strict';
  const ids = Object.freeze(['red', 'orange', 'yellow', 'green', 'blue', 'violet']);
  function parse(document) {
    if (document?.format !== 'subcubes-material-palette' || document.version !== 1 || document.colorSpace !== 'sRGB')
      throw new Error('Expected subcubes-material-palette v1 in sRGB.');
    const input = document.materials;
    if (!Array.isArray(input) || input.length !== ids.length || new Set(input.map(m => m?.id)).size !== ids.length)
      throw new Error('Expected six unique material IDs.');
    const normalized = n => typeof n === 'number' && Number.isFinite(n) && n >= 0 && n <= 1;
    const materials = Object.freeze(ids.map(id => {
      const m = input.find(m => m?.id === id);
      if (!m || typeof m.name !== 'string' || !m.name.trim() || !['r', 'g', 'b'].every(k => normalized(m.rgb?.[k])) || !normalized(m.roughness) || !normalized(m.metallic))
        throw new Error(`Invalid material: ${id}.`);
      const rgb = Object.freeze({r: m.rgb.r, g: m.rgb.g, b: m.rgb.b});
      const hex = '#' + Object.values(rgb).map(n => Math.round(n * 255).toString(16).padStart(2, '0')).join('');
      return Object.freeze({id, name: m.name, rgb, roughness: m.roughness, metallic: m.metallic, hex});
    }));
    return Object.freeze({materials, byId: Object.freeze(Object.fromEntries(materials.map(m => [m.id, m]))), fingerprint: JSON.stringify(materials)});
  }
  async function load(url) {
    const response = await fetch(url, {cache: 'no-store'});
    if (!response.ok) throw new Error(`Palette request failed (${response.status}).`);
    return parse(await response.json());
  }
  if (typeof module === 'object' && module.exports) { module.exports = {parse}; return; }
  const url = new URL('subcubes-materials.json', document.currentScript.src);
  // Local file:// pages may prohibit fetch. Let the editor load the same JSON
  // explicitly in that case; never silently substitute a second colour table.
  const ready = load(url).catch(error => new Promise(resolve => {
    const panel = document.createElement('div');
    panel.style.cssText = 'position:fixed;inset:16px 16px auto;z-index:10000;padding:16px;background:#fff;color:#222;border:1px solid #888;border-radius:8px';
    const label = document.createElement('label'), status = document.createElement('p'), input = document.createElement('input');
    label.textContent = 'Choose subcubes-materials.json to load the shared palette. ';
    input.type = 'file'; input.accept = '.json,application/json';
    status.textContent = error.message;
    input.addEventListener('change', async () => {
      if (!input.files[0]) return;
      try { const palette = parse(JSON.parse(await input.files[0].text())); panel.remove(); resolve(palette); }
      catch (error) { status.textContent = error.message; }
    });
    label.append(input); panel.append(label, status); document.body.append(panel);
  }));
  root.SubCubesPalette = Object.freeze({parse, ready});
})(globalThis);
