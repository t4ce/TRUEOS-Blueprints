# QuickJS headless compatibility lab

A source copy of [`apps/qjs`](../qjs) for executing captured website JavaScript
without WebKit. It runs a native Linux QuickJS engine with a small host surface
and an in-memory DOM. The production qjs app and kernel ABI are unchanged.

From the Blueprint repository root:

```sh
cargo build --manifest-path apps/qjs-web-lab/Cargo.toml --bin qjs-web-lab
python3 -m unittest discover -s apps/qjs-web-lab -v
python3 apps/qjs-web-lab/run.py
python3 apps/qjs-web-lab/run.py --replay --site wikipedia
python3 apps/qjs-web-lab/run.py --refresh-modules --site github
```

The runner requires Linux, Python 3 and Bubblewrap (`bwrap`). Fresh dependency
capture also requires Node.js with `require('acorn')` available. Acorn parses
source into an AST outside the VM; it does not execute website code. Missing
parser support remains a capture issue. Replay uses saved sources without network
access. `--refresh-modules` keeps the saved HTML/root scripts and fetches their
module dependencies again.

Cargo's repository configuration produces
`target/x86_64-unknown-linux-gnu/debug/qjs-web-lab`. QuickJS is pinned to commit
`2833510432045f72915919cef680ba59e5153b16`; the copied bindings and C header use
the same 16-byte PTR64 value ABI. Host compilation retains native Atomics and
stack checks.

## DistanceOnTheMonitor

[`src/dotm.js`](src/dotm.js) defines the immutable `DistanceOnTheMonitor` datatype
and its `DotM` alias. This lab explicitly uses **1 DotM = 1 cm** as its reference
convention. A DotM is a physical monitor distance, not a raster cell, CSS pixel,
DPI value, or a dimensionless glyph proportion. The headless process has no
measured display and performs no device-pixel conversion.

The glyph's normalized height is `1`, advance is `6/11`, and alphabetic ascent
and descent are `9/11` and `2/11`. Those ratios have no distance unit. Applying
them to the lab's reference em of `1.1 DotM` yields `0.6 DotM` natural advance.
A different typed em changes physical size while preserving the ratios:

```js
const em = DotM.fromCentimeters(1.1);
const measured = MicrofontMetrics.measure('abc', em);
measured.width.toCentimeters(); // approximately 1.8
DotM.fromNormalized(1, em);     // 1.1 DotM
```

Distances require explicit conversion (`toNumber`, `toCentimeters`,
`toMillimeters`) and typed arithmetic; implicit numeric coercion throws.
`toJSON` retains `{value, unit: 'DotM'}`. Canvas `measureText` and DOM geometry
APIs explicitly unwrap distances to numbers for calling scripts, and report their
metric space as DotM. `canvasContext.monitorEm` and `element.monitorEm` accept
a typed distance. Font family names and CSS font declarations can be retained
without downloading or rendering vector fonts; their numeric CSS syntax is not
silently converted through a fixed-DPI assumption.

The browser-facing viewport uses an explicit synthetic headless monitor profile,
not a detected monitor. Its dimensions and provenance are included in the report.
Canvas draw records retain site-supplied coordinates separately from text metrics;
those input coordinates are not presented as calibrated display measurements.

The existing repositories establish the relevant boundaries:

- [Hull text measurement](../../../TRUEOS-Buildins/hull/src/layout.rs) uses
  Unicode-scalar count times font size times `6/11`.
- [Microfont](../../../TRUEOS/vendor/microfont/src/lib.rs) stores a `6×11` cell
  representation. Optional special-cell expansion is distinct from ordinary Hull
  text counting.
- [The kernel font service](../../../TRUEOS/src/r/services/hull_font_service.rs)
  applies the same natural advance and `9/11` baseline, with a physical size ladder.
- [Display metrics](../../../TRUEOS/src/intel/display/display_metrics.rs) convert
  physical extents using validated EDID dimensions and the active display mode.

DotM is introduced in this lab; existing fields such as the kernel bridge's
`size_px` and `advance_px` retain their existing meanings. Connecting a future
DotM presentation boundary must use the monitor-calibrated kernel conversion.

## Host execution and isolation

Each page gets a fresh runtime and process. Website scripts execute, then the
runner dispatches `DOMContentLoaded`/`load` and pumps timers with a microtask drain
between task turns. Delays use virtual time; `performance.now()` uses the host's
monotonic clock. Timeouts, intervals, animation-frame/idle callbacks, in-memory
storage/cookies and console capture are available. Same-runtime `MessageChannel`
delivers structured-cloned messages as tasks; transfer lists and workers remain
unsupported. The history API retains cloned state and supports task-based
`popstate`; URL changes/reloads are recorded without navigation.
Intersection/resize observers retain targets and explicitly report
inactivity because there is no layout-driven visibility/resize service. `crypto.getRandomValues` and
`randomUUID` use host entropy through a bounded native callback. There is no guest
filesystem, process API, `fetch`, or XHR binding.

The VM has an eight-second execution deadline, a 256 MiB QuickJS heap limit,
a stack limit and bounded task/job processing. The controller also imposes process
resource limits and a wall timeout. Bubblewrap removes network access and mounts
system runtime files, the executable, private `/proc` and `/dev`, and empty temporary
storage. It does not mount the repository, home directory, credentials, or browser
profile. Controller builds/downloads run outside the execution sandbox.

## Lazy DOM and text behavior

[`src/virtual_dom.js`](src/virtual_dom.js) maintains node identity, attributes,
classes/styles, text, tree mutations, fragments, shadow roots, templates, event
listeners and a basic custom-element registry. Structure is created on demand.
Single-node queries can create a synthetic placeholder; multiple-node queries
return actual matches. Supported selectors cover simple IDs, classes, tags and
attributes. Complex selectors and synthetic nodes are recorded.

Mutation observers receive batched microtask notifications for implemented tree,
attribute and character-data changes. Event/listener errors are recorded while
subsequent listeners can continue. Custom-element constructor upgrades and full
browser event/lifecycle semantics remain incomplete.

[`src/web_compat.js`](src/web_compat.js) provides text measurement, a minimal
2D canvas context, FontFace/FontFaceSet availability, and inactive animations.
Font requests map to microfont; no font files are fetched. Canvas text calls retain
the string and DotM metrics. Raster drawing calls are recorded without painting.
WebGL contexts return unavailable. Images do not download or report a fictional
successful load. `innerHTML` retains its string and records an approximation; it
does not parse markup into a full document tree.

## Capture, modules and reports

The controller captures public HTML and executable script tags. Acorn discovers
static imports, re-exports and literal dynamic imports. Import maps support exact
and prefix mappings with scopes. [`src/offline_modules.rs`](src/offline_modules.rs)
resolves and compiles captured dependencies inside QuickJS without network or
filesystem access. Computed dynamic targets and dynamically inserted script tags
are not a complete browser loading system; uncaptured/unresolved modules remain
visible failures.

Capture limits are 256 root script tags, 256 dependency modules, dependency depth
12, 16 MiB per resource, and 64 MiB total source text. Failures and limit hits are
retained as incomplete captures. Root scripts run in HTML order; browser async/defer
ordering, workers, networking and complete browser layout/rendering are outside
this implementation.

Ignored `artifacts/<site>/` contains the captured HTML, sources, hashes, parser
version, dependency resolutions and execution report. `--replay` reuses those
sources. `baseline.json` is a checked-in summary; compare its capture times and
script/module counts before treating two runs as equivalent.

[`src/api_trace.js`](src/api_trace.js) counts instrumented host/DOM method calls and
property reads/writes, with a bounded 200-call history containing the execution
phase and abbreviated arguments. This is not a count of every JavaScript function.
Reports also contain DOM allocation/approximation notes, text records, module
requests, console messages, exceptions, rejected promises and pending work.
Exception reports include the available preceding API-call history.

`clean` means only that the captured execution produced no reported exceptions,
rejected promises or console warnings/errors and no recorded incomplete work.
It does not validate a working website. Unsupported selectors, inactive animation,
font remapping and other approximations remain visible even when execution is
clean. Inaccessible pages, uncaptured dependencies and unfinished task queues must
not be counted as passing sites.

The current checked-in baseline has clean captured startup execution for Wikipedia
and Google. YouTube reaches DOM Range support; GitHub/mail reach missing initial
page configuration. Discord still hits the script-count cap and a WebAssembly
dependency. crates.io and DeepSeek remain capture failures. These are pending
compatibility tasks, not passing websites. The fixture suite currently has 32 tests.

## Chrome event comparison

`compare_chrome.mjs` drives a fresh Chrome for Testing instance through its local
DevTools endpoint. It requires Node.js with `ws`; use a new temporary browser
profile, not an existing signed-in profile. This run used Chrome
`156.0.8078.80` from the official Chrome for Testing release manifest. The downloaded
browser and profile are outside the repository.

```sh
node compare_chrome.mjs --mode captured-offline --port-file /path/to/temporary-profile/DevToolsActivePort
node compare_chrome.mjs --mode live --port-file /path/to/temporary-profile/DevToolsActivePort
node compare_chrome.mjs --mode live --site youtube,github,crates --no-probe --port-file /path/to/temporary-profile/DevToolsActivePort
python3 compare_events.py
python3 compare_events.py --refresh-chrome-only
```

The preload probe records actual listener callbacks, observers, scheduling, API
activity, and connected DOM/text changes. It separates JS factory returns from
HTML parser nodes and excludes detached shadow roots from connected totals. Raw
Unicode text includes scripts/styles; content-only counts are separate and neither
means rendered text. Instrumentation is observable; receiver/listener, scheduler
and observer behavior were checked against native Chrome.

Chrome reports distinguish baseline diagnostics from errors caused by later
synthetic events. Each QuickJS candidate uses a fresh runtime; Chrome's sequential
probes also record a before snapshot and an idle control. Synthetic events are
untrusted and scheduled work may still run during Chrome's observation window.
Capture hashes verify saved-source provenance, not equivalent execution: Chrome
parses the full saved HTML, including inline handlers and possibly inline scripts
beyond the controller's admission cap; QuickJS has a synthetic initial tree.

The optional QuickJS payload field `lifecycle_mode: "chrome"` tests an approximate
readystatechange → DOMContentLoaded (including window delivery) → complete/load/
pageshow sequence, with promise jobs between tasks. Default `legacy` behavior is
unchanged. Unknown modes are rejected. This experiment is not browser conformance.

The checked-in `chrome_comparison.json` contains the compact comparison. Full
reports are ignored under `artifacts/chrome-comparison/`, including the causal
`lifecycle-mode-comparison.json`. On the six captured sites, the Chrome-like
sequence caused no additional connected nodes or text. YouTube delivered three
more callbacks but retained its existing startup failure. Repeating an already
delivered DOMContentLoaded caused a new Chrome YouTube error because its original
handler deletes its resolver. The next useful work is preserving initial HTML/
configuration and implementing remaining API boundaries, rather than firing more
load events indiscriminately.
