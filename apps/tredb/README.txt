tredb 0.1.0
===========

A deliberately small TrueOS terminal application for exploring a redb database
that exists entirely in RAM.

It is not a database server and has no client protocol. The application embeds
upstream redb directly and uses redb::backends::InMemoryBackend. Closing the app
discards the database.

Dependency policy
-----------------
- direct crossterm UI; no Ratatui
- upstream redb 4.2.0; redb is not vendored or patched
- redb builds with default features off plus experimental-api-5
- that combination activates redb's experimental no_std build
- redb still uses alloc; the terminal application itself uses std
- no SQL, network service, async runtime, serde, or persistence layer

TrueOS placement
----------------
Place the directory where these Texplo-style paths resolve:

    ../../api
    ../../vendor/crossterm-0.29.0-trueos
    ../../vendor/mio-1.2.0
    ../../vendor/rustix-1.1.4-trueos
    ../../vendor/signal-hook-mio-0.2.5-trueos

Rust 1.90 or newer is required by redb 4.2.0. Then run:

    cargo run

Launch directives
-----------------
The optional TRUEOS vFile:launch script understands:

    seed demo
    seed empty

Command-line forms:

    cargo run -- --demo
    cargo run -- --empty

Controls
--------
- LMB: select a database, table, row, or menu item
- Arrow keys or W/A/S/D: pan the graph
- Mouse wheel: vertical pan
- Home: center the graph
- Tab / Shift+Tab: cycle DB, View, and Action menu sections
- 0..9: run the numbered item in the active menu section
- Enter: context default (new table, new row, or edit value)
- Delete: delete selected table or row, after confirmation
- R: refresh the owned UI snapshot from redb
- H or ?: show help
- Esc: close a modal; with no modal, exit
- Ctrl-Q: exit

Data convention
---------------
Every table created by tredb uses redb's byte-slice key/value types:

    TableDefinition<&[u8], &[u8]>

Input is UTF-8 by default. Prefix an input with "hex:" to enter bytes, for
example:

    hex:00 ff 7a

Printable UTF-8 is shown as text. Other bytes are shown in the same hex form.
This explicit convention is what keeps the explorer generic without inventing
a schema or codec plugin system.

Native smoke-test manifest
--------------------------
Cargo.native.toml swaps the TrueOS path dependencies for crates.io crossterm.
Use it only in a scratch copy:

    cp Cargo.toml Cargo.trueos.toml
    cp Cargo.native.toml Cargo.toml
    cargo run

Restore Cargo.trueos.toml before placing the app in the TrueOS tree.

Architecture
------------
- src/store.rs: the only redb-facing module
- src/model.rs: owned snapshots and stable selections
- src/layout.rs: database/table/row graph layout and hit rectangles
- src/screen.rs: retained terminal-cell framebuffer and diff renderer
- src/menu.rs: local numbered menu sections
- src/modal.rs: text entry and confirmation state
- src/app.rs: input dispatch, transactions, drawing, and event batching
- src/main.rs: configuration and terminal lifecycle

The UI never stores redb guards, tables, or transactions. A user action opens a
short transaction, commits, then rebuilds an owned snapshot. This keeps redb
lifetimes out of the retained application state.
