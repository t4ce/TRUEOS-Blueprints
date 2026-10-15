#!/usr/bin/env python3
"""Compile production error classification and check logging field privacy."""
from pathlib import Path
import re
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
source = (HERE / 'src/main.rs').read_text()
for literal in re.findall(r'"(http phase=[^"\n]+)"', source):
    assert not any(word in literal for word in ['url=', 'header=', 'body=', 'error='])
    assert set(re.findall(r'(\w+)=', literal)) <= {
        'phase', 'state', 'endpoint', 'elapsed_ms', 'is_timeout', 'is_connect',
        'is_request', 'source_depth', 'source_truncated', 'io_kind', 'tls_class',
        'source_classes', 'status_code',
    }
assert 'String::from("HTTP send failed; see fixed diagnostic categories")' in source
with tempfile.TemporaryDirectory(prefix='appstore-http-diagnostics-') as directory:
    path = Path(directory)
    (path / 'test.rs').write_text(f'#[path="{HERE}/src/http_diagnostics.rs"] mod diagnostics;')
    subprocess.run(['rustc', '--edition=2024', '--test', str(path/'test.rs'), '-o', str(path/'test')], check=True)
    subprocess.run([str(path/'test')], check=True)
