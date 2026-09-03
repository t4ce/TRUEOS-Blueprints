# TRUEOS BIOS

Read-only browser UI for the BIOS schema exported by TRUEOS.

The Blueprint is intentionally just a localhost web service:

```text
http://127.0.0.1:1012/
```

There is no browser dependency in the Blueprint. Today the service can be exercised through the HTTP surface; a future TRUEOS browser can open the same localhost URL without changing the BIOS application model.

## Runtime data

`GET /api/bios/schema` reads the kernel's immutable `trueos_vlayer_bios_schema_snapshot_read` surface.

The UI supports both generations of the schema:

- `trueos-bios-schema/v1`: form sets, forms and validated questions.
- ordered presentation snapshots: when `presentation.nodes` is present, the UI renders source-order `SUBTITLE`, `TEXT`, `REF`, question and Tiano-label structure and falls back to v1 questions otherwise.

No board-specific `bios.txt` is embedded in the app. The hardware dump was used to shape the renderer, not as runtime state.

## HTTP boundary

Only GET routes exist:

```text
/
 /index.html
 /app.js
 /app.css
 /healthz
 /api/healthz
 /api/bios/schema
```

The listener binds to `127.0.0.1` only.

There is no endpoint for save, submit, callback, variable routing, reset or firmware mutation. F10 and Ctrl/Cmd+S are blocked in the browser as a UI affordance; the stronger boundary is the kernel snapshot ABI, which is read-only.

Current firmware values remain redacted/not decoded.
