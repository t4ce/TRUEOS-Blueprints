# Local Conrod fork

Based on https://gitlab.com/veloren/conrod.git, branch `copypasta_0.7`,
commit `c7444636` (the revision previously pinned by Veloren's Cargo.lock).

Only conrod_core and conrod_derive are vendored. Original licenses are retained.

The copypasta dependency is removed. TextEdit uses the public
`conrod_core::clipboard::{read, write}` API backed by a process-local text buffer.
Veloren's Iced UI uses the same buffer. There is no OS clipboard integration:
text can be copied between game fields, but not between the game and other apps.
Clipboard contents are discarded when the game exits.

Numeric widgets depend directly on `num-traits 0.2` rather than the `num 0.2`
umbrella crate. Only trait import paths change; widget calculations are unchanged.
