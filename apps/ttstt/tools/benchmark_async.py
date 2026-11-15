#!/usr/bin/env python3
"""Compare the Ubuntu smol full-model harness; no builds or native publication."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
import wave


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--coordinator", type=int, default=0)
    parser.add_argument("--compute-cpus", default="2,8,10")
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--occupancy", action="store_true", help="enable optional per-thread kernel/CPU tracing")
    args = parser.parse_args()
    cpus = [int(value) for value in args.compute_cpus.split(",")]
    if len(cpus) != 3 or len(set(cpus + [args.coordinator])) != 4:
        parser.error("choose three distinct compute CPUs and a separate coordinator")
    if not set(cpus + [args.coordinator]) <= os.sched_getaffinity(0):
        parser.error("requested CPUs unavailable")
    if args.repeats < 2:
        parser.error("at least two interleaved passes required")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve()
    result = {
        "host_cpu": next(line.split(":", 1)[1].strip() for line in
                         Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
        "binary": str(binary), "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
        "coordinator_cpu": args.coordinator, "compute_cpus": cpus,
        "runtime": "smol 2.0.2; fixed executor threads; bounded async channels; awaited operator completion",
        "metric": "elapsed calculation including parked time awaiting workers, excluding warmup",
        "occupancy_tracing": args.occupancy,
        "runs": [],
    }
    reference = {}
    for repeat in range(args.repeats):
        order = [0, 2, 1, 3] if repeat % 2 == 0 else [3, 1, 2, 0]
        for workers in order:
            directory = out / f"pass-{repeat}-workers-{workers}"
            log = out / f"pass-{repeat}-workers-{workers}.log"
            started = time.monotonic()
            environment = os.environ.copy()
            if args.occupancy:
                environment["TTSTT_OCCUPANCY"] = "1"
            else:
                environment.pop("TTSTT_OCCUPANCY", None)
            process = subprocess.run(
                [str(binary), str(workers), str(args.coordinator),
                 ",".join(map(str, cpus[:workers])), str(directory)],
                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=360, env=environment,
            )
            log.write_text(process.stdout)
            if process.returncode:
                raise RuntimeError(f"workers={workers}: {process.returncode}; {log}")
            record = json.loads((directory / "result.json").read_text())
            completed = [int(value) for value in re.findall(r"job=\d+ done compute_ms=(\d+)", process.stdout)]
            if len(completed) != len(record["cases"]) + 1:
                raise RuntimeError("missing warmup or measured completion notices")
            record.update(repeat=repeat, log=str(log), process_seconds=time.monotonic()-started)
            for case, compute in zip(record["cases"], completed[1:]):
                path = Path(case["wav"])
                with wave.open(str(path), "rb") as audio:
                    assert (audio.getframerate(), audio.getnchannels(), audio.getsampwidth()) == (48000, 2, 2)
                    assert audio.getnframes() > 0
                digest = hashlib.file_digest(path.open("rb"), "sha256").hexdigest()
                if case["case"] not in reference:
                    reference[case["case"]] = digest
                assert reference[case["case"]] == digest, f"PCM changed: {case['case']} workers={workers}"
                assert abs(case["synthesis_ms"]-compute) < max(30, case["synthesis_ms"]*0.01)
                case.update(compute_ms=compute, wav_sha256=digest)
            if workers:
                assert record["operations"] == 696 and record["tiles"] > 0
            assert record["heartbeat_ticks"] > 0
            result["runs"].append(record)
            (out / "results.json").write_text(json.dumps(result, indent=2)+"\n")
            print(json.dumps({"repeat": repeat, "workers": workers,
                  "compute_ms": {case["case"]: case["compute_ms"] for case in record["cases"]},
                  "max_worker_tile_us": record["max_worker_tile_us"],
                  "coordinator_max_gap_us": record["coordinator_max_gap_us"],
                  "all_wav_hashes_equal": True}), flush=True)
    result["summary"] = {}
    for name in reference:
        modes = {}
        for workers in range(4):
            times = [case["compute_ms"] for run in result["runs"] if run["workers"] == workers
                     for case in run["cases"] if case["case"] == name]
            modes[str(workers)] = {"compute_ms": times, "median_ms": statistics.median(times)}
        for mode in modes.values():
            mode["less_time_percent"] = 100*(1-mode["median_ms"]/modes["0"]["median_ms"])
        result["summary"][name] = modes
    result["all_wav_hashes_equal"] = True
    (out / "results.json").write_text(json.dumps(result, indent=2)+"\n")
    print(json.dumps(result["summary"], indent=2), flush=True)


if __name__ == "__main__":
    main()
