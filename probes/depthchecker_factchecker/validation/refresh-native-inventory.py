#!/usr/bin/env python3
"""Refresh the curated capture candidates from the actual packed debug ELF.

The existing JSON defines the candidate selection; this script verifies every
named symbol and replaces offsets, sizes and artifact hashes. It does not infer
runtime hits from symbol presence.
"""

import hashlib
import json
from pathlib import Path
import subprocess


PROBE = Path(__file__).resolve().parents[1]
BLUEPRINTS = PROBE.parents[1]
DIST = BLUEPRINTS / "dist"
INVENTORY = PROBE / "validation/native-elf-symbols.json"


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    inventory = json.loads(INVENTORY.read_text())
    binding = json.loads((DIST / "depthchecker_factchecker.debug.json").read_text())
    elf = Path(binding["debug_elf"]["path"])
    for key in ("blueprint", "debug_elf", "packed_elf"):
        record = binding[key]
        path = Path(record["path"])
        assert path.stat().st_size == record["bytes"], f"size changed: {path}"
        assert sha256(path) == record["sha256"], f"hash changed: {path}"

    symbols = {}
    output = subprocess.check_output(["nm", "-S", str(elf)], text=True)
    for line in output.splitlines():
        parts = line.split()
        if len(parts) == 4:
            offset, size, kind, symbol = parts
            symbols[symbol] = {
                "symbol": symbol,
                "section": ".text" if kind.lower() == "t" else ".bss",
                "section_offset": hex(int(offset, 16)),
                "size": int(size, 16),
                "nm_type": kind,
            }
        elif len(parts) == 2 and parts[0] == "U":
            symbols[parts[1]] = {
                "symbol": parts[1], "section": "UNDEFINED", "nm_type": "U"
            }

    def refresh(rows):
        for index, previous in enumerate(rows):
            current = symbols[previous["symbol"]]
            assert current["nm_type"].lower() in ("t", "b", "u"), current
            rows[index] = current

    refresh(inventory["markers"])
    refresh(inventory["atomics"])
    for scenario in inventory["scenarios"]:
        for key in ("selected_targets", "additional_candidates", "nonselected_or_inline_alternatives"):
            if key in scenario:
                refresh(scenario[key])

    worker = next(s for s in inventory["scenarios"] if s["id"] == 6)
    names = {s["symbol"] for s in worker["selected_targets"]}
    for name in ("fsync", "read", "write", "trueos_cabi_async_fs_remove_start", "trueos_cabi_poll_once"):
        if name not in names:
            worker["selected_targets"].append(symbols[name])
    worker["reason"] = (
        "Parent interval spans spawn/join; the worker repeats real parsing on a "
        "separate native stack, then performs the same 35-byte filesystem roundtrip. "
        "The filesystem calls can select native continuation parking; that indirect "
        "kernel branch requires a runtime hit with genuine worker ancestry. "
        "Imported service addresses belong to the matching kernel ELF."
    )
    worker["indirect_kernel_candidate"] = {
        "symbol": "trueos_thread_context_swap",
        "resolution": "matching kernel ELF",
        "scope": "Source-backed native continuation branch; not a Blueprint import or static proof of a hit.",
    }
    inventory["artifact_binding"] = binding
    source = PROBE / "src/main.rs"
    overlay = PROBE / "target/trueos-blueprint/work/depthchecker_factchecker/source-overlay-app/src/main.rs"
    entry_suffix = """
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern \"C\" fn _start() -> ! {
    let _ = main();
    unsafe extern \"C\" {
        fn exit(code: core::ffi::c_int) -> !;
    }
    unsafe { exit(0) }
}
"""
    expected_overlay = "#![no_main]\n" + source.read_text() + entry_suffix
    assert overlay.read_text() == expected_overlay, "staged main.rs differs beyond packer entry augmentation"
    inventory["source_binding"] = {
        "main_rs_sha256": sha256(source),
        "staged_main_rs_sha256": sha256(overlay),
        "staging_transform": "Packer adds #![no_main] and its _start/exit entry wrapper; remaining source is exact.",
        "native_worker_filesystem_roundtrip": True,
    }
    INVENTORY.write_text(json.dumps(inventory, indent=2) + "\n")
    print(json.dumps({"inventory": str(INVENTORY), "blueprint": binding["blueprint"]}))


if __name__ == "__main__":
    main()
