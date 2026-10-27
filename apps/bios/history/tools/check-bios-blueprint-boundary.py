#!/usr/bin/env python3
"""Mechanical read-only guard for the BIOS Blueprint."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "apps" / "bios"
SERVER = (APP / "server.rs").read_text(encoding="utf-8")
JS = (APP / "app.js").read_text(encoding="utf-8")
README = (APP / "README.md").read_text(encoding="utf-8")

for token in (
    'SocketAddr::from(([127, 0, 0, 1], PORT))',
    '"/api/bios/schema", get(schema)',
    'v::vbios::snapshot_bytes()',
):
    if token not in SERVER:
        raise SystemExit(f"missing BIOS localhost/read-only contract: {token}")

for forbidden in (
    "routing::post",
    ".post(",
    ".put(",
    ".patch(",
    ".delete(",
    "set_variable",
    "route_config",
    "firmware_write",
):
    if forbidden.casefold() in SERVER.casefold():
        raise SystemExit(f"BIOS Blueprint write path detected: {forbidden}")

for token in (
    'event.key === "F10"',
    'activeWritePath !== "none"',
    "question_match=none",
):
    if token not in JS:
        raise SystemExit(f"missing BIOS UI safety/rendering contract: {token}")

if "No board-specific `bios.txt` is embedded" not in README:
    raise SystemExit("README must retain the dynamic firmware-data boundary")

print("bios-blueprint-boundary: localhost GET-only BIOS explorer verified")
