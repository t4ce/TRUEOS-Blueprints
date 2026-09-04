# TRUEOS BIOS

Read-only firmware setup explorer served by the TRUEOS Blueprint web stack.

The application is intentionally a normal Axum service. It has no browser dependency; today it can be opened from another machine, and a future TRUEOS browser/WebView can open the same service without changing the BIOS application model.

## Runtime data

`GET /api/bios/schema` passes through the immutable kernel BIOS snapshot.

The renderer understands both generations:

- `trueos-bios-schema/v3`: read-only platform/runtime facts alongside the v2 HII and presentation data.
- `trueos-bios-schema/v2`: semantic form/question records plus source-ordered `presentation.nodes`.
- `trueos-bios-schema/v1`: fallback rendering of validated questions when presentation nodes are unavailable.

The v2 presentation stream drives the visible firmware layout: `SUBTITLE`, `TEXT`, `REF`, question/action records and Tiano label metadata remain in firmware source order. The semantic question graph remains available for storage, options, defaults, policy and engineering details.

No board-specific `bios.txt` is embedded in the application.

## UI model

The primary interface follows firmware-setup navigation rather than a schema debugger:

- top-level categories are derived from captured formsets and controller identity;
- forms and cross-form references become firmware pages;
- settings are dense rows in source order;
- firmware help is contextual;
- engineering IDs, varstores and policy metadata live in the optional Details drawer;
- current values are explicitly `Not exposed`, never confused with defaults.

`Main` is a read-only platform page. It composes SMBIOS firmware/board identity, UEFI handoff state and discovered PCI controllers without promoting any of them into firmware settings. All other categories remain derived from captured formsets; the application does not invent Advanced/Chipset/Boot tabs when those formsets are absent from the captured HII database.

## Read-only boundary

Only GET routes exist. There is no save, submit, callback, variable routing, reset or firmware mutation endpoint. Firmware actions are displayed as unavailable records. F10 and Ctrl/Cmd+S are blocked as a UI affordance; the stronger boundary is the kernel ABI, which has no mutation operation.
