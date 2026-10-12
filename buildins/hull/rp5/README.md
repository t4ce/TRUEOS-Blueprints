# rp5

Native Rust translation of the parse5 parser vendored next door in `../p5`.
Upstream revision: `103d9aa6b50a903ba23134124337b2201b3822c2` (parse5 8.0.1).

The library takes decoded Unicode and builds a document or context-sensitive
fragment in a Rust arena. The arena preserves stable node identities during
formatting repair and table foster parenting, stores template content in its
own fragment, and avoids recursive node destruction. It has no runtime crate
dependencies, JavaScript engine, sockets, or TLS implementation.

```rust
let document = rp5::parse("<!doctype html><title>Hello</title><p>A &amp; B");
assert_eq!(document.title().as_deref(), Some("Hello"));
```

`Hull::poll_download` decodes the kernel-downloaded bytes as UTF-8, calls `rp5`,
and retains the document. This stage builds the HTML tree; it does not perform
CSS layout, rendering, or script execution.

The port translates the one-shot tokenizer and tree construction path. Streaming
input, custom tree adapters, and parse5's source-location API are outside this
initial API. Invalid UTF-8 replacement is handled by Hull before parsing;
HTTP charset and HTML encoding sniffing are not part of `parse(&str)`.
Diagnostics include tokenizer errors and selected tree-recovery errors. Their
offsets use normalized character positions (tree-recovery offsets are currently
zero); complete parse5 diagnostics and original source spans are not exposed.

Validation:

```sh
cargo test --manifest-path Cargo.toml
node tests/parse5_oracle.mjs --check
python3 tools/generate_tables.py
node tools/verify_entities.mjs
```

`tests/fixtures.json` is a shared native/JavaScript corpus. The oracle compares
complete trees, namespaces, attributes, template content, and document modes.
Named-reference data contains all 2,231 WHATWG entries and is checked against
the vendored entities 8.1.0 decoder. SVG adjustments and quirks-mode prefixes
are generated directly from the vendored parse5 TypeScript.

The broader differential corpus uses html5lib-tests revision
`a9f44960a9fedf265093d22b2aa3c7ca123727b9`, the submodule revision pinned by
the vendored parse5 source. Generate it outside the repository:

```sh
git clone https://github.com/html5lib/html5lib-tests.git /tmp/html5lib-tests
git -C /tmp/html5lib-tests checkout a9f44960a9fedf265093d22b2aa3c7ca123727b9
python3 tools/import_html5lib.py /tmp/html5lib-tests /tmp/rp5-html5lib.json
node tests/parse5_oracle.mjs --fixtures /tmp/rp5-html5lib.json --report /tmp/rp5-report.json
```

This produces 8,504 inputs: 1,545 documents, 192 tree-construction fragments,
and 6,767 tokenizer inputs used in matching fragment content modes. Expected
trees come from the checked-in parse5 bundle, so these checks validate complete
native trees rather than the isolated token sequences recorded upstream. The
importer skips 27 scripting-disabled cases, 261 initial tokenizer states that
cannot be selected through this fragment API, and four unpaired-surrogate
inputs that cannot be represented by Rust UTF-8 strings. The oracle runs Cargo
from a temporary directory to avoid Blueprint workspace build-std settings.

For isolated tokenizer verification, run:

```sh
python3 tools/test_tokenizer.py /tmp/html5lib-tests/tokenizer
```

All 7,028 supported cases match the upstream tokens and error codes across
data, RCDATA, raw text, script, plaintext, and CDATA states. Four inputs with
unpaired UTF-16 surrogates and optional XML coercion fixtures are excluded.

See `LICENSE` for parse5's MIT license and `LICENSE.entities` for the entity
decoder's BSD-2-Clause license. The entity table generator uses Python's
WHATWG HTML5 reference data as an enumeration source and verifies every value
against the vendored decoder.
