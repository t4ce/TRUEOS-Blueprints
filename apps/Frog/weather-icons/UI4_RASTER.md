# Frog UI4 weather rasters

`ui4-jpeg/64` and `ui4-jpeg/128` contain the UI4-safe raster form of the
top-level `animated` icon set. The directories are deliberately flat. Each
icon uses this stable naming scheme:

```text
clear-day-frame-000.jpg
clear-day-frame-001.jpg
...
clear-day-frame-009.jpg
```

Frame `000` is rendered from `static/<icon>.svg`. It is therefore a stable,
non-animated bring-up asset even though the remaining frames are sampled from
`animated/<icon>.svg`. Frog can initially consume only frame `000` and retain a
single UI4 surface until its normal weather refresh. The other frames are an
optional low-rate sprite sequence for a later iteration.

Each frame `000` also has a same-name `.rgba` sidecar. It contains headerless,
row-major RGBA8 pixels with an opaque alpha byte: 65,536 bytes at 128px and
16,384 bytes at 64px. These sidecars let the kernel-facing UI4 renderer embed
the initial icon without introducing a JPEG decoder. Animated sidecars are not
generated; the JPEG sequence remains the compact source for that later step.

All JPEGs are square RGB images on a `#142238` matte because JPEG cannot carry
the SVGs' transparency. The generator removes the source SVGs' large internal
padding and fits the static first frame inside a 112x96 content area. Animated
frames share one union content bound per icon so their motion does not jitter.
The normalized 128px canvas is downsampled for the 64px set.

## Rebuild

Run from this directory:

```sh
tools/build-ui4-jpegs.sh
```

The build requires Google Chrome or Chromium for phase-controlled CSS/SMIL
animation sampling, plus ImageMagick for JPEG encoding. Each icon's longest
declared animation duration becomes its sampling period, capped at ten evenly
spaced frames. `ui4-jpeg/manifest.tsv` records the period and stable first-frame
JPEG and RGBA8 paths.

For a first-frame-only development bundle:

```sh
UI4_WEATHER_FRAME_COUNT=1 tools/build-ui4-jpegs.sh
```

The frame count is constrained to 1 through 10. Set
`UI4_WEATHER_BACKGROUND` to six hexadecimal RGB digits to choose a different
JPEG matte, and set `CHROME` to override the browser executable.
