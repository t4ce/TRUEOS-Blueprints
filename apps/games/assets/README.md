# Chess Deco cubes

These six CUBES v1 files are one-time exports of the `deco` models in
`Cubes/Cube/AssetShowcase.html` (`cube-chess-core`). The reproducible exporter is
`../tools/export_chess_deco.cjs`. Each file retains every authored unit cube;
the single neutral palette color is a placeholder for the two chess materials.

The rubric offers both Horse and armored Knight but no Bishop. Games uses Horse
for the chess knight and armored Knight for the bishop. The other four roles
map directly. The Games renderer derives a c3 exposed-cube view from each full
asset to fit all 32 pieces in the TRUEOS retained-seed budget, while keeping
one scale and baseline across the collection.
