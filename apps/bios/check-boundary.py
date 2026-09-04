#!/usr/bin/env python3
"""Mechanical read-only/presentation guard for apps/bios."""
from pathlib import Path

APP = Path(__file__).resolve().parent
server = (APP / "server.rs").read_text(encoding="utf-8")
js = (APP / "app.js").read_text(encoding="utf-8")
html = (APP / "index.html").read_text(encoding="utf-8")

for token in ('"/api/bios/schema", get(schema)', 'v::vbios::snapshot_bytes()'):
    if token not in server:
        raise SystemExit(f"missing BIOS snapshot route: {token}")

for forbidden in ("routing::post", ".post(", ".put(", ".patch(", ".delete(", "set_variable", "route_config", "firmware_write"):
    if forbidden.casefold() in server.casefold():
        raise SystemExit(f"write-capable BIOS server surface detected: {forbidden}")

for token in (
    'key:"main"',
    's.platform||{}',
    's.runtime||{}',
    'presentation?.nodes',
    'name==="subtitle"',
    'name==="text"',
    'name==="ref"',
    'S.schema?.current?.questions',
    'c?.status==="decoded"',
    'visibilityFor(q)==="suppressed"',
    'Captured preboot',
    'e.key==="F10"',
    'question_match=none',
    's.activeWritePath!=="none"',
):
    if token not in js:
        raise SystemExit(f"missing ordered/read-only UI contract: {token}")

for token in ("top-tabs", "help-rail", "details-drawer"):
    if token not in html:
        raise SystemExit(f"missing firmware UI surface: {token}")

print("bios-ui-boundary: GET-only ordered firmware renderer with captured current state verified")
