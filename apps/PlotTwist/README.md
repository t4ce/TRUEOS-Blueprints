# PlotTwist

PlotTwist is a small, server-authoritative Axum game blueprint for two to four
human players. A player gets four figures and one mathematical plot per
60-second turn. A plotted trace removes enemy figures whose 1920×1080 collision
pixels overlap it; generated blocks interrupt traces, and ally damage is off.

## Implemented rules

- Nickname plus one of five Twemoji presets, or an uploaded PNG/JPEG/WebP.
  The browser center-crops and downsizes uploads to a 64×64 WebP.
- Open lobby browser; 2–4 players per lobby; empty lobbies are removed.
- Teams 1–4 and unrestricted HTML color picker. Everyone on a team shares its
  most recently selected color.
- All players ready starts a ten-second countdown. Unready, lobby setting
  changes, joins, and leaves cancel and fully reset it.
- Seeded software RNG generates seven blocking boxes and four non-overlapping
  figures per player in the bounded `[-500, 500] × [-500, 500]` X/Z arena.
- One-minute turns, pause/resume, end game, lobby chat, three measurement pins
  per player turn, transient plot traces, elimination, and team victory.
- Canvas fly camera with WASD, Q/E height, Shift boost, drag-look, a hard
  `Y >= 5` camera floor, and click-to-measure ground ray casting.

The compact expression language accepts explicit plots such as
`y = sin(x / 35) * 120`, vertical plots such as `x = y^2 / 100`, approximate
implicit plots such as `x^2 + y^2 = 10000`, and the non-function helpers
`circle(cx, cy, r)` and `segment(x1, y1, x2, y2)`. Supported functions are
`sin`, `cos`, `tan`, `abs`, `sqrt`, `ln`, `log`, `exp`, `floor`, and `ceil`.

## Build and test

From the `TRUEOS-Blueprints` root:

```sh
cargo test -p PlotTwist --lib
cargo bp PlotTwist
```

The portable library contains the lobby/game state machine, parser, seeded
generation, rasterization, and hit testing. The TRUEOS-only binary embeds the
browser assets and serves the API on blueprint port 8338 using the lifecycle
listener.

## Art attribution

The five presets load SVG graphics from
[Twemoji 14.0.2](https://github.com/twitter/twemoji/tree/v14.0.2), copyright
Twitter, Inc. and other contributors, licensed under
[CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/). Uploaded player art
remains the responsibility of its uploader.
