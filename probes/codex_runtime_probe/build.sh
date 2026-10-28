#!/usr/bin/env bash
set -euo pipefail
probe_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
blueprint_root=$(cd -- "$probe_root/../.." && pwd)
cd -- "$blueprint_root"
export TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1
export CARGO_NET_OFFLINE=true
cargo +nightly-2026-07-10 run --offline --bin trueos-blueprint -- "$probe_root"
