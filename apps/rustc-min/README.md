# rustc-min source database

The app source lives in `TRUEOS-Blueprints/apps/rustc-min`; compiler dependencies
remain pinned to the sibling `TRUEOS-Rust-Toolchain-nightly-2026-07-10` archive.
From the Blueprint repository, run `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp rustc-min`.
The compiler app remains excluded from bulk app packing.

`rustc-min` keeps immutable application source revisions in its app-private
`rustc-sources.redb` database. The database contains source file bytes,
paths, modes, content hashes, and revision identity only. Compiler objects and
Blueprint executables are never inserted into it.

The former experimental `rustc-sources.sqlite3` file is deliberately left
untouched. Re-import its source archives into the redb store; rustc-min no
longer links a SQLite reader or migration runtime.

## Commands

```text
rustc-min source import-7z <project> <archive.7z>
rustc-min source import-git [project] <github-commit-link> [exact-commit]
rustc-min source list [project]
rustc-min source delete <project> <revision>
rustc-min source delete <project> --all
rustc-min source export-7z <project> <revision|latest> <archive.7z>
rustc-min source compile <project> [revision|latest] [--entry path] [--output path.bp]
rustc-min source verify
```

A Git import requires a full 40- or 64-character object ID. GitHub
`/commit/<oid>`, `/tree/<oid>`, `repository#<oid>`, and
`repository <oid>` forms are accepted. Repository URL and object ID are
canonicalized before the network request, so importing the same exact commit
returns the existing database revision without downloading it again. The first
provider adapter uses GitHub's tree/blob HTTPS API through the existing TRUEOS
Hyper service.

Local 7z imports always create a new immutable revision. Import and export use
the kernel async archive service and a unique app-local staging directory; the
staging tree is removed when the operation finishes.

## Compilation boundary

For database builds, rustc receives a virtual `FileLoader` backed by the chosen
redb revision. Paths inside that virtual source root cannot fall through to
the physical filesystem. The embedded target, sysroot, and dependency metadata
remain filesystem-backed.

The compiler emits one transient ELF relocatable object, validates its
architecture, entry point, imports, and relocations, wraps it in a TRBP v1
Blueprint, writes the Blueprint to the filesystem, then deletes the object.
The default output is `/common/localcompile/<project>.bp`, the existing Shell2
local-compile handoff directory. `--output` can select another caller-visible
path.

The current publisher intentionally accepts one ELF REL object. Projects that
need a native linker or multiple codegen objects remain outside this first
database compiler path.
