#!/usr/bin/env python3
"""Compare resolved kernel/app Cargo graphs without changing packaging.

Supply `cargo metadata --locked --format-version 1 --filter-platform <target>`
JSON for each side. For an actual pack, use the staged app manifest with the
packer's patch configs and feature flags; the original workspace graph is only
a preliminary alignment audit. Kernel metadata must use the kernel build's
features. Metadata features can be unified across workspace/host consumers and
are not proof that two compilation units have identical features or ABI.

Example (saved metadata):
  python3 tools/dependency_economy.py --kernel-metadata kernel.json \
      --app-metadata apps.json --package image-viewer --package webmail
  # Use --all-workspace-packages instead of --package to inspect every member.

The normal dependency closure includes registry, git, vendor and local crates,
using Cargo package IDs rather than dependency aliases or folder names. Build
dependencies, dev dependencies, proc macros and disabled optional dependencies
are not runtime candidates. An unfiltered input can include other platforms.
Nothing in this report authorizes stripping: same-version crates still need
an exported loader contract and compatible compilation/ownership semantics.
"""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sys


class Graph:
    def __init__(self, metadata):
        self.packages = {p["id"]: p for p in metadata["packages"]}
        resolve = metadata.get("resolve")
        if resolve is None:
            raise ValueError("metadata needs a resolved graph (omit --no-deps)")
        self.nodes = {n["id"]: n for n in resolve["nodes"]}

    def root(self, name):
        matches = [p for p in self.packages.values() if p["name"] == name or p["id"] == name]
        if len(matches) != 1:
            raise ValueError(f"package {name!r} matches {len(matches)} packages; select an exact package ID")
        return matches[0]["id"]

    def closure(self, root):
        seen = set()
        pending = [root]
        while pending:
            package_id = pending.pop()
            if package_id in seen:
                continue
            if package_id not in self.packages or package_id not in self.nodes:
                raise ValueError(f"incomplete metadata for {package_id}")
            seen.add(package_id)
            for edge in self.nodes[package_id]["deps"]:
                if not any(kind["kind"] is None for kind in edge["dep_kinds"]):
                    continue
                dependency = self.packages[edge["pkg"]]
                # A proc-macro package may also have a custom-build target.
                if any("proc-macro" in target["kind"] for target in dependency["targets"]):
                    continue
                pending.append(edge["pkg"])
        return seen

    def describe(self, package_id):
        package = self.packages[package_id]
        return {
            "id": package_id,
            "name": package["name"],
            "version": package["version"],
            "source": package["source"],
            "manifest_path": package["manifest_path"],
            "resolved_features": sorted(self.nodes[package_id]["features"]),
        }


def audit(kernel_metadata, app_metadata, kernel_package, app_packages):
    kernel = Graph(kernel_metadata)
    apps = Graph(app_metadata)
    kernel_root = kernel.root(kernel_package)
    kernel_ids = kernel.closure(kernel_root) - {kernel_root}
    by_name = defaultdict(list)
    for package_id in sorted(kernel_ids):
        by_name[kernel.packages[package_id]["name"]].append(package_id)

    reports = []
    for name in app_packages:
        app_root = apps.root(name)
        app_ids = apps.closure(app_root) - {app_root}
        rows = []
        for package_id in sorted(app_ids, key=lambda i: (apps.packages[i]["name"], i)):
            app = apps.describe(package_id)
            candidates = by_name.get(app["name"], [])
            same_version = [i for i in candidates if kernel.packages[i]["version"] == app["version"]]
            if package_id in same_version:
                status = "same-package"
            elif same_version:
                status = "same-version-other-source"
            elif candidates:
                status = "version-mismatch"
            else:
                status = "app-only"
            rows.append({
                "app": app,
                "status": status,
                "kernel_candidates": [
                    {
                        **kernel.describe(i),
                        "resolved_features_equal": app["resolved_features"] == kernel.describe(i)["resolved_features"],
                    }
                    for i in candidates
                ],
            })
        reports.append({
            "package": apps.describe(app_root),
            "runtime_dependency_count": len(rows),
            "counts": dict(sorted(Counter(row["status"] for row in rows).items())),
            "dependencies": rows,
        })
    return {
        "schema_version": 1,
        "scope": "resolved normal dependencies; target filtering and pack overlays depend on supplied metadata",
        "automatic_strip_supported": False,
        "kernel": kernel.describe(kernel_root),
        "kernel_runtime_dependency_count": len(kernel_ids),
        "kernel_runtime_dependencies": [kernel.describe(i) for i in sorted(kernel_ids)],
        "apps": reports,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--kernel-metadata", type=Path, required=True)
    parser.add_argument("--app-metadata", type=Path, required=True)
    parser.add_argument("--kernel-package", default="TRUEOS")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--package", action="append", help="app package name or exact Cargo package ID")
    selection.add_argument("--all-workspace-packages", action="store_true", help="audit every member of the supplied app workspace graph")
    args = parser.parse_args()
    try:
        app_metadata = json.loads(args.app_metadata.read_text())
        app_packages = sorted(app_metadata["workspace_members"]) if args.all_workspace_packages else args.package
        if not app_packages:
            raise ValueError("the app metadata contains no workspace members")
        report = audit(
            json.loads(args.kernel_metadata.read_text()),
            app_metadata,
            args.kernel_package,
            app_packages,
        )
        report["inputs"] = {
            "kernel_metadata": str(args.kernel_metadata.resolve()),
            "app_metadata": str(args.app_metadata.resolve()),
        }
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"dependency-economy: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
