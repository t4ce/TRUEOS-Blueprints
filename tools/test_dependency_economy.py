"""Regressions for cases where a dependency overlap could incorrectly imply reuse."""

import unittest

from dependency_economy import Graph, audit


def package(package_id, name, version="1.0.0", kinds=None):
    return {
        "id": package_id, "name": name, "version": version,
        "source": None, "manifest_path": f"/{package_id}/Cargo.toml",
        "targets": [{"kind": kind} for kind in (kinds or [["lib"]])],
    }


def metadata(packages, edges=None, features=None):
    edges = edges or {}
    features = features or {}
    return {
        "packages": packages,
        "resolve": {"nodes": [
            {"id": p["id"], "features": features.get(p["id"], []), "deps": [
                {"name": alias, "pkg": child, "dep_kinds": [{"kind": kind, "target": None}]}
                for alias, child, kind in edges.get(p["id"], [])
            ]}
            for p in packages
        ]},
    }


class EconomyTests(unittest.TestCase):
    def test_aliases_and_transitive_local_packages_use_resolved_identity(self):
        packages = [package("root", "root"), package("fs", "trueos-fs"), package("hash", "hashbrown")]
        graph = Graph(metadata(packages, {
            "root": [("renamed_fs", "fs", None), ("same_fs_again", "fs", None)],
            "fs": [("hash", "hash", None)],
        }))
        self.assertEqual(graph.closure("root"), {"root", "fs", "hash"})

    def test_build_dev_disabled_and_proc_macro_dependencies_are_excluded(self):
        packages = [package("root", "root"), package("build", "build"), package("dev", "dev"),
                    package("disabled", "optional"), package("macro", "derive", kinds=[["proc-macro"], ["custom-build"]]),
                    package("macro_helper", "macro-helper"), package("proc_macro2", "proc-macro2")]
        graph = Graph(metadata(packages, {
            "root": [("build", "build", "build"), ("dev", "dev", "dev"),
                     ("derive", "macro", None), ("parser", "proc_macro2", None)],
            "macro": [("helper", "macro_helper", None)],
        }))
        self.assertEqual(graph.closure("root"), {"root", "proc_macro2"})

    def test_multiple_versions_source_forks_and_features_are_separate(self):
        kernel = metadata([package("kernel", "TRUEOS"), package("old", "bytes"),
                           package("new", "bytes", "2.0.0"), package("shared", "sha2")], {
            "kernel": [("old", "old", None), ("new", "new", None), ("sha", "shared", None)],
        }, {"shared": ["std"]})
        app = metadata([package("app", "app"), package("fork", "bytes"),
                        package("drift", "bytes", "3.0.0"), package("shared", "sha2")], {
            "app": [("alias", "fork", None), ("v3", "drift", None), ("sha", "shared", None)],
        }, {"shared": ["alloc"]})
        report = audit(kernel, app, "TRUEOS", ["app"])
        rows = {row["app"]["id"]: row for row in report["apps"][0]["dependencies"]}
        self.assertEqual(rows["fork"]["status"], "same-version-other-source")
        self.assertEqual(rows["drift"]["status"], "version-mismatch")
        self.assertEqual(len(rows["fork"]["kernel_candidates"]), 2)
        self.assertEqual(rows["shared"]["status"], "same-package")
        self.assertFalse(rows["shared"]["kernel_candidates"][0]["resolved_features_equal"])
        self.assertFalse(report["automatic_strip_supported"])

    def test_ambiguous_names_and_missing_resolve_fail(self):
        graph = Graph(metadata([package("one", "same"), package("two", "same", "2.0.0")]))
        with self.assertRaises(ValueError):
            graph.root("same")
        self.assertEqual(graph.root("one"), "one")
        with self.assertRaises(ValueError):
            Graph({"packages": [], "resolve": None})


if __name__ == "__main__":
    unittest.main()
