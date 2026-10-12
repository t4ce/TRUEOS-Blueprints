# parse5 in Hull

Source: https://github.com/inikulin/parse5
Commit: 103d9aa6b50a903ba23134124337b2201b3822c2
Package version: 8.0.1

`src/` contains the upstream `packages/parse5/lib` source, unchanged.
`parse5.js` is its bundled JavaScript, including entities 8.1.0. It has no
Node.js or network dependency and exposes the global `parse5`, including
`parse5.parse(html)` and `parse5.parseFragment(html)`.

Rebuild: `npm ci && npm run build`
Verify: `npm test`

The parser is vendored for later JavaScript runtime integration. Hull currently
downloads HTML through the kernel into memory; it does not execute this bundle.
The upstream MIT license and entity-decoder BSD-2-Clause license are included alongside the bundle.
