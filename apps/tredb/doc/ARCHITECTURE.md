# tredb architecture

`tredb` is one process and one address space:

```text
crossterm events
      |
      v
 retained TrueOS terminal UI
      |
      v
 short redb read/write transactions
      |
      v
 redb::backends::InMemoryBackend
      |
      v
 RAM only
```

## Hard boundaries

The storage module owns `redb::Database`. The rest of the program sees only
owned `DbSnapshot`, `TableSnapshot`, and `RowSnapshot` values. No redb guard or
transaction crosses the storage boundary.

The visual model uses one stable `Selection` value. After every mutation the
snapshot is rebuilt and the selection is normalized against the new snapshot.

## Why byte tables only

redb records Rust type information, but a generic application cannot decode an
arbitrary application's custom `Value` implementation. `tredb` therefore owns a
small public convention: all user tables are `&[u8] -> &[u8]`. Text and `hex:`
input are only views over those bytes.

## Why redb is no_std but the app is not

The dependency declaration disables redb's `std` feature and enables its API-5
preview. redb's build script then selects its no_std implementation, backed by
`alloc`. The application still uses `std` for terminal I/O, event timing, and
TrueOS integration. That split removes filesystem assumptions from the database
engine without pretending a terminal app needs to be `#![no_std]`.

## Deliberately absent

There is no SQL parser, server protocol, file format browser, persistence toggle,
codec registry, plugin host, background worker, or async runtime.
