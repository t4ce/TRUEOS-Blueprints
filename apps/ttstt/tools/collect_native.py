#!/usr/bin/env python3
"""Read-only collection of completed TRUEOS shared-convolution measurements."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import statistics
import urllib.parse
import urllib.request

ORACLE = {
    "short": "091da6e08944ad69065b19c1fb8e755818eac390bc027d91803894665c9e5599",
    "medium": "ab6579b1b17f6bb7619fad42180e27dfe86e19e3c5708916214022cf99c1b5ab",
    "long": "55167877128099b9ec8acc368d27982ba8eb10f1c5f3d9941a6e228dec7b1552",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="http://192.168.178.94")
    parser.add_argument("--root", type=int, required=True, help="root ID discovered through GET /")
    parser.add_argument("--nonce", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,48}", args.nonce):
        parser.error("invalid benchmark nonce")
    base = args.base.rstrip('/') + f"/dl/{args.root}/apps/ttstt/benchmarks/{args.nonce}/"
    def download(name):
        with urllib.request.urlopen(base+urllib.parse.quote(name, safe=''),timeout=30) as response:
            return response.read()
    result = json.loads(download("result.json"))
    assert result["complete"] and result["nonce"] == args.nonce and result["target"] == "trueos"
    args.output.mkdir(parents=True,exist_ok=True)
    summary = {}
    for row in result["rows"]:
        name = row["wav"]
        assert Path(name).name == name and name.endswith('.wav')
        blob = download(name)
        digest = hashlib.sha256(blob).hexdigest()
        assert digest == ORACLE[row["case"]], f"Original WAV mismatch: {name}"
        assert len(blob) == 44+row["frames"]*4
        (args.output/name).write_bytes(blob)
        row["wav_sha256"] = digest
        if row["workers"] == 3:
            assert row["local_fallbacks"] == 0, f"Native admission fell back: {name}"
            assert row["peak_remote_workers"] == 3
            assert row["operations"] == {"short":87,"medium":174,"long":435}[row["case"]]
            assert row["placements"] and all(p["lane"]=="avx-vnni" for p in row["placements"])
        else:
            assert row["workers"] == 0 and row["operations"] == row["remote_jobs"] == 0
    for name in ORACLE:
        cases = [r for r in result["rows"] if r["case"]==name]
        if not cases:
            continue
        modes = {}
        for workers in [0,3]:
            rows = [r for r in cases if r["workers"]==workers]
            assert {r["repeat"] for r in rows} == {0,1} and len(rows)==2
            values = [r["elapsed_us"]/1e6 for r in rows]
            modes[str(workers)] = {"elapsed_s":values,"median_s":statistics.median(values)}
        modes["less_time_percent"] = 100*(1-modes["3"]["median_s"]/modes["0"]["median_s"])
        summary[name] = modes
    result["summary"] = summary
    result["all_original_wavs_match"] = True
    result["all_observed_workers_performance"] = all(p["core_kind"]=="Performance" for r in result["rows"] for p in r["placements"])
    (args.output/"result.json").write_text(json.dumps(result,indent=2)+'\n')
    with (args.output/"summary.csv").open('w',newline='') as file:
        writer=csv.writer(file)
        writer.writerow(['case','serial_median_s','three_workers_median_s','less_time_percent'])
        for name,modes in summary.items():
            writer.writerow([name,modes['0']['median_s'],modes['3']['median_s'],modes['less_time_percent']])
    print(json.dumps({'summary':summary,'all_original_wavs_match':True,
                      'all_observed_workers_performance':result['all_observed_workers_performance'],
                      'wav_files':len(result['rows'])},indent=2))


if __name__ == '__main__':
    main()
