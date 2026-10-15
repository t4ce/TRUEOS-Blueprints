#!/bin/sh
set -eu
probe_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$probe_dir/../.."
exec env TRUEOS_BLUEPRINT_PROBES_PUBLISH_URI='smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps' \
    cargo bpp mcp_http_probe
