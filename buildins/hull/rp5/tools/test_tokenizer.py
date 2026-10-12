#!/usr/bin/env python3
"""Run rp5 against the external html5lib tokenizer fixtures.

Use the parse5-pinned html5lib-tests checkout, commit
 a9f44960a9fedf265093d22b2aa3c7ca123727b9:
  python3 tools/test_tokenizer.py /path/to/html5lib-tests/tokenizer

All six initial states, token output and parse-error codes are compared.
Optional XML coercion fixtures are excluded because parse5 does not implement
that mode. Isolated UTF-16 surrogate inputs are excluded because Rust &str
represents valid Unicode. Source locations are not compared.
"""
import argparse
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("fixtures", type=Path, help="html5lib-tests/tokenizer directory")
args = parser.parse_args()
fixtures = args.fixtures.resolve()
if not fixtures.is_dir():
    parser.error(f"not a directory: {fixtures}")
crate = Path(__file__).resolve().parents[1]
environment = dict(os.environ, RP5_HTML5LIB_TOKENIZER=str(fixtures))
raise SystemExit(subprocess.run([
    "cargo", "test", "--manifest-path", str(crate / "Cargo.toml"), "--lib",
    "tokenizer::tests::html5lib_tokenizer_fixture_corpus", "--", "--ignored", "--nocapture",
], env=environment).returncode)
