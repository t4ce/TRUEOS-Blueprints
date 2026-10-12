#!/usr/bin/env python3
"""Convert upstream html5lib inputs into rp5's parse5 differential fixtures.

Usage:
    python3 tools/import_html5lib.py /path/to/html5lib-tests /tmp/rp5-html5lib.json
    node tests/parse5_oracle.mjs --fixtures /tmp/rp5-html5lib.json --check

Use html5lib-tests commit a9f44960a9fedf265093d22b2aa3c7ca123727b9 to match
the vendored parse5 revision. This imports input and fragment context only;
parse5 supplies the expected tree, including changes since upstream .dat files.
Tokenizer inputs exercise matching fragment content modes. Their expected trees
come from parse5, rather than html5lib's isolated token sequences; fragments do
not seed a last-start-tag token, so context closing tags can remain text.
Errors, streaming boundaries, script execution, scripting-disabled cases, and
isolated UTF-16 surrogates are outside rp5's one-shot UTF-8 API.
"""

import argparse
from collections import Counter
import json
from pathlib import Path
import re


def rust_string(value):
    """Combine UTF-16 surrogate pairs, but reject unpaired surrogate inputs."""
    try:
        return value.encode("utf-16-le", "surrogatepass").decode("utf-16-le")
    except UnicodeDecodeError:
        return None


def unicode_unescape(value):
    return re.sub(r"\\u([0-9a-fA-F]{4})", lambda match: chr(int(match[1], 16)), value)


def dat_records(path):
    sections = None
    section = None
    line_number = None
    for number, line in enumerate(re.split(r"\r?\n", path.read_bytes().decode("utf-8")), 1):
        if line == "#data":
            if sections is not None:
                yield line_number, sections
            sections = {}
            line_number = number
        if sections is None:
            continue
        if line.startswith("#"):
            section = line
            sections[section] = []
        else:
            sections[section].append(line)
    if sections is not None:
        yield line_number, sections


def import_trees(root, counts):
    for path in sorted((root / "tree-construction").glob("*.dat")):
        for line, record in dat_records(path):
            if "#script-off" in record:
                counts["skipped_scripting_disabled"] += 1
                continue
            if "#disable-html-entities-decoding" in record:
                counts["skipped_entity_decoding_disabled"] += 1
                continue
            html = rust_string("\n".join(record["#data"]))
            if html is None:
                counts["skipped_unpaired_surrogate"] += 1
                continue
            fixture = {"name": f"html5lib_tree_{path.stem}_L{line}", "mode": "document", "html": html}
            fragment = record.get("#document-fragment")
            if fragment:
                parts = fragment[0].split(" ")
                if len(parts) == 1:
                    namespace, tag = "html", parts[0]
                else:
                    namespace, tag = {"svg": "svg", "math": "mathml"}[parts[0]], parts[1]
                fixture["mode"] = "fragment"
                fixture["context"] = {"tag": tag, "namespace": namespace}
                counts["tree_fragments"] += 1
            else:
                counts["tree_documents"] += 1
            yield fixture


def tokenizer_context(state, last_start_tag):
    if state == "Data state":
        return {}
    if state == "RCDATA state" and last_start_tag in {"title", "textarea"}:
        return {"tag": last_start_tag, "namespace": "html"}
    if state == "RAWTEXT state" and last_start_tag in {"style", "xmp", "iframe", "noembed", "noframes", "noscript"}:
        return {"tag": last_start_tag, "namespace": "html"}
    if state == "Script data state" and last_start_tag == "script":
        return {"tag": "script", "namespace": "html"}
    if state == "PLAINTEXT state":
        return {"tag": "plaintext", "namespace": "html"}
    return None


def import_tokens(root, counts):
    for path in sorted((root / "tokenizer").glob("*.test")):
        tests = json.loads(path.read_text(encoding="utf-8")).get("tests", [])
        for number, test in enumerate(tests, 1):
            html = test["input"]
            if test.get("doubleEscaped"):
                html = unicode_unescape(html)
            html = rust_string(html)
            if html is None:
                counts["skipped_unpaired_surrogate"] += 1
                continue
            for state in test.get("initialStates", ["Data state"]):
                context = tokenizer_context(state, test.get("lastStartTag", ""))
                if context is None:
                    counts["skipped_unrepresentable_initial_state"] += 1
                    continue
                state_name = state.removesuffix(" state").replace(" ", "_").lower()
                fixture = {
                    "name": f"html5lib_token_{path.stem}_{number}_{state_name}",
                    "mode": "fragment",
                    "html": html,
                }
                if context:
                    fixture["context"] = context
                counts["tokenizer_inputs"] += 1
                yield fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="html5lib-tests repository root")
    parser.add_argument("output", type=Path, help="output fixture JSON file")
    parser.add_argument("--only", choices=["trees", "tokens", "all"], default="all")
    args = parser.parse_args()
    counts = Counter()
    fixtures = []
    if args.only in {"trees", "all"}:
        fixtures.extend(import_trees(args.source, counts))
    if args.only in {"tokens", "all"}:
        fixtures.extend(import_tokens(args.source, counts))
    assert len({fixture["name"] for fixture in fixtures}) == len(fixtures)
    args.output.write_text(json.dumps(fixtures, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"fixtures": len(fixtures), **counts}, indent=2))


if __name__ == "__main__":
    main()
