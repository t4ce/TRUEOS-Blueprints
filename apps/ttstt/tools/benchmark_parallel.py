#!/usr/bin/env python3
"""Compare isolated full-model host experiments, verifying the original WAV oracle."""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import statistics
import subprocess
import time
import wave

ORACLE = {
    "short": "091da6e08944ad69065b19c1fb8e755818eac390bc027d91803894665c9e5599",
    "medium": "ab6579b1b17f6bb7619fad42180e27dfe86e19e3c5708916214022cf99c1b5ab",
    "long": "55167877128099b9ec8acc368d27982ba8eb10f1c5f3d9941a6e228dec7b1552",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--variants", default="serial:0,channels:2,shared:2,branches:2,channels:3,shared:3,branches:3")
    parser.add_argument("--coordinator", type=int, default=0)
    parser.add_argument("--compute-cpus", default="2,8,10")
    parser.add_argument("--repeats", type=int, default=2)
    args = parser.parse_args()
    cpus = [int(n) for n in args.compute_cpus.split(",")]
    variants = []
    for text in args.variants.split(","):
        mode, count = text.split(":")
        count = int(count)
        if mode not in ("serial", "channels", "shared", "branches") or not 0 <= count <= len(cpus):
            parser.error("invalid variant")
        if (mode == "serial") != (count == 0):
            parser.error("only serial must have zero workers")
        variants.append((mode, count))
    if args.repeats < 1 or len(set(variants)) != len(variants):
        parser.error("positive repetitions and distinct variants required")
    if len(set(cpus + [args.coordinator])) != len(cpus)+1:
        parser.error("compute CPUs and coordinator must be distinct")
    if not set(cpus + [args.coordinator]) <= os.sched_getaffinity(0):
        parser.error("requested CPUs unavailable")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve()
    result = {"binary": str(binary), "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
              "host_cpu": next(s.split(":",1)[1].strip() for s in Path("/proc/cpuinfo").read_text().splitlines() if s.startswith("model name")),
              "coordinator": args.coordinator, "compute_cpus": cpus, "repeats": args.repeats,
              "occupancy": False, "metric": "full synthesis wall time, warmed; original complete model",
              "runs": []}
    for repeat in range(args.repeats):
        order = variants if repeat % 2 == 0 else list(reversed(variants))
        for mode, workers in order:
            name = f"{mode}-{workers}"
            directory = output/f"pass-{repeat}-{name}"
            env = os.environ.copy()
            env.pop("TTSTT_OCCUPANCY", None)
            before = resource.getrusage(resource.RUSAGE_CHILDREN)
            started = time.monotonic()
            process = subprocess.run([str(binary), str(workers), str(args.coordinator),
                                      ",".join(map(str,cpus[:workers])), str(directory),
                                      "channels" if mode == "serial" else mode],
                                     capture_output=True, text=True, env=env, timeout=600)
            after = resource.getrusage(resource.RUSAGE_CHILDREN)
            log = output/f"pass-{repeat}-{name}.log"
            log.write_text(process.stdout+process.stderr)
            if process.returncode:
                raise RuntimeError(f"{name} failed: {process.returncode}; {log}")
            run = json.loads((directory/"result.json").read_text())
            completed = [int(n) for n in re.findall(r"job=\d+ done compute_ms=(\d+)", process.stderr)]
            assert len(completed) == len(run["cases"])+1, "missing synthesis completion notices"
            for case, compute in zip(run["cases"],completed[1:]):
                wav = Path(case["wav"])
                with wave.open(str(wav),"rb") as audio:
                    assert (audio.getframerate(),audio.getnchannels(),audio.getsampwidth()) == (48000,2,2)
                    assert audio.getnframes() > 0
                digest = hashlib.file_digest(wav.open("rb"),"sha256").hexdigest()
                assert digest == ORACLE[case["case"]], f"PCM changed: {name} {case['case']}"
                assert abs(case["synthesis_ms"]-compute) < max(30,case["synthesis_ms"]*0.01)
                case.update(compute_ms=compute,wav_sha256=digest)
            assert run["heartbeat_ticks"] > 0
            if mode in ("shared","channels"):
                assert run["operations"] == 696
            if mode == "branches":
                assert run["operations"] == 0, "branch experiment must not share individual convolutions"
                assert run["graph"]["operations"] == 16 and run["graph"]["branches"] == 48
            run.update(variant=name,experiment=mode,repeat=repeat,log=str(log),
                       process_wall_s=time.monotonic()-started,
                       process_cpu_s_including_warmup=after.ru_utime+after.ru_stime-before.ru_utime-before.ru_stime)
            result["runs"].append(run)
            (output/"results.json").write_text(json.dumps(result,indent=2)+"\n")
            print(json.dumps({"pass":repeat,"variant":name,
                              "synthesis_ms":{c["case"]:round(c["synthesis_ms"],2) for c in run["cases"]},
                              "exact_original_wavs":True}),flush=True)
    result["summary"] = {}
    for name in ORACLE:
        modes = {}
        for mode, workers in variants:
            key = f"{mode}-{workers}"
            values = [c["synthesis_ms"] for r in result["runs"] if r["variant"]==key for c in r["cases"] if c["case"]==name]
            modes[key] = {"synthesis_ms":values,"median_ms":statistics.median(values)}
            cpu = [c["process_cpu_ms"] for r in result["runs"] if r["variant"]==key for c in r["cases"]
                   if c["case"]==name and "process_cpu_ms" in c]
            if len(cpu) == len(values):
                modes[key].update(process_cpu_ms=cpu,median_process_cpu_ms=statistics.median(cpu),
                                  mean_active_process_cores=statistics.median(cpu)/statistics.median(values))
        if "serial-0" in modes:
            for row in modes.values():
                row["less_time_percent"] = 100*(1-row["median_ms"]/modes["serial-0"]["median_ms"])
        result["summary"][name] = modes
    result["all_match_original_wav_oracle"] = True
    (output/"results.json").write_text(json.dumps(result,indent=2)+"\n")
    with (output/"summary.csv").open("w",newline="") as file:
        writer = csv.writer(file)
        writer.writerow(["case","variant","median_wall_s","median_process_cpu_s","less_time_percent"])
        for name, modes in result["summary"].items():
            for variant, row in modes.items():
                cpu = row.get("median_process_cpu_ms")
                writer.writerow([name,variant,row["median_ms"]/1000,cpu/1000 if cpu is not None else "",
                                 row.get("less_time_percent","")])
    print(json.dumps(result["summary"],indent=2),flush=True)


if __name__ == "__main__":
    main()
