#!/usr/bin/env python3
import argparse
from pathlib import Path
from common import Layout, initialize

p = argparse.ArgumentParser()
p.add_argument('--address', required=True)
p.add_argument('--root', type=Path, default=Path('/var/lib/w3box'))
p.add_argument('--prefix', type=Path, default=Path('/opt/w3box'))
a = p.parse_args()
password = initialize(Layout(a.root, a.prefix), a.address)
if password:
    # Installer captures this in a root-only file; never send it to the install log.
    print(password)
