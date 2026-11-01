# QuickJS headless compatibility lab

An isolated source copy of `apps/qjs`, with a Linux host runner for testing website
JavaScript without WebKit. The original app is unchanged. This fork keeps the
copied runtime modules for reference; its host manifest builds the native engine
and headless runner, not the TRUEOS workbench.

From the Blueprint repository root:

```sh
cargo build --manifest-path apps/qjs-web-lab/Cargo.toml --bin qjs-web-lab
python3 -m unittest discover -s apps/qjs-web-lab -v
python3 apps/qjs-web-lab/run.py
python3 apps/qjs-web-lab/run.py --replay --site wikipedia
```

Requires Linux, Python 3 and Bubblewrap (`bwrap`). The build follows the repository's
Cargo configuration, producing `target/x86_64-unknown-linux-gnu/debug/qjs-web-lab`.
The engine is pinned to QuickJS commit `2833510432045f72915919cef680ba59e5153b16`.
Its header is adjusted to the 16-byte value ABI used by the copied Rust bindings;
host compilation retains native Atomics and stack checks.

## Execution model

The Python controller captures public HTML and external script tags. Captures,
resource hashes and per-site reports are saved in the ignored `artifacts/` directory.
Replay uses those exact sources without downloading again. Network failures and
capture limits remain explicit failures/incomplete runs.

Each page gets a fresh process and runtime, with an eight-second execution deadline,
a 256 MiB QuickJS heap limit, and bounded jobs, timers and logs. Bubblewrap removes
network access and mounts only system runtime files, the VM executable, private
`/proc`, `/dev` and an empty temporary directory. It does not mount the repository,
home directory, credentials or browser profile. JavaScript has no filesystem,
process or network bindings. Build/fetch operations run outside that execution sandbox.

The initial host functions are in `src/web_host.js`: console capture, queued timers
with virtual delays, microtasks, location, a basic navigator and per-page in-memory
storage. Storage is discarded between runs. No `fetch`, XHR, crypto, or filesystem
API has been added yet.

## Lazy DOM

`src/virtual_dom.js` implements a small in-memory tree. Only the document exists
initially; HTML/head/body and other nodes are allocated when requested. Nodes keep
identity, attributes, classes, text, child order, parent links, event listeners,
and tree mutations. Layout reads return zero; images do not download or fire a
fake successful load event. Computed-style reads return stored values or empty strings.

For this lab, a missing single-node query (`getElementById` or `querySelector`)
creates and remembers a synthetic placeholder. Multiple-node queries return actual
matches. Selector support currently covers simple IDs, classes, tags and attributes.
Complex selectors and synthetic nodes are reported. `innerHTML` currently stores its
string and records an approximation; it does not parse markup into nodes. This is a
requested headless approximation, not browser DOM conformance. Unknown methods are
left visible as errors so they can be implemented deliberately.

## Reports and remaining work

A `clean` execution means only that the captured scripts produced no reported
exceptions, rejected promises or console warnings/errors. It does not mean the site
works. Reports explicitly mark website functionality as unvalidated, and list DOM
approximations. Nothing suppresses site warnings to manufacture a pass.

Scripts run in HTML order. Module dependency graphs, import maps, async/defer
scheduling, dynamically inserted scripts, networking, observers, and browser lifecycle
semantics are not complete. The controller records capture failures; the engine
reports module-load errors. These are incremental implementation tasks.

`baseline.json` summarizes the first reproducible capture/replay run. Current
blockers include crypto, additional DOM APIs, module loading and HTTP access errors.
Use the saved reports to implement one missing host function, add a focused fixture,
then replay all eight pages. Do not treat inaccessible pages as passing tests.
