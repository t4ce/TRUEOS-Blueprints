# Server source import

Imported on 2026-10-06 from `/home/t4ce/Programmme/veloren-srvlocal`,
base commit `29793e001158a06013fd2bc1c290d1b8e985ca47`, including the working-tree
changes present there. The original checkout is preserved.

This tree carries the server, CLI, world generation, shared workspace crates,
client support crates, local dependency forks, and the complete asset tree.
Build outputs, userdata, Git history, and the graphical client are excluded;
its i18n helper is retained for workspace dependency resolution.
`GAME_VERSION` is materialized from the original symlink target.

The workspace omits graphical-client members and GPU patches. The TRUEOS asset
adapter points to this repository's `api`. `apps/velosrv` uses this tree's local
dependencies and holds a regular copy of `server-cli/src` for Blueprint staging.
When changing CLI code, keep those two source trees synchronized.

Original licensing is preserved in `LICENSE` and the crate manifests.
