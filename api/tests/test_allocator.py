#!/usr/bin/env python3
"""Run the shared guarded allocator contract suite against this SDK's source.

The harness extracts TrueosAllocator and native ABI declarations verbatim. Its
guarded heap injects allocation failures and records every exact pointer/layout;
the no_std object probe rejects a return to retired C allocation imports.
"""

import importlib.util
from pathlib import Path
import sys
import unittest


SDK_ROOT = Path(__file__).resolve().parents[2]
SHARED_TOOLS = SDK_ROOT.parent / "TRUEOS-Buildins" / "tools"
HARNESS = SHARED_TOOLS / "test_sdk_allocator.py"
if not HARNESS.is_file():
    raise RuntimeError(f"missing shared allocator contract harness: {HARNESS}")
sys.path.insert(0, str(SHARED_TOOLS))
spec = importlib.util.spec_from_file_location("standalone_sdk_allocator_contract", HARNESS)
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
shared.SDK = SDK_ROOT / "api/src/lib.rs"
shared.ABI = SDK_ROOT / "crates/trueos-v/src/bp_abi.rs"


if __name__ == "__main__":
    unittest.main(module=shared)
