#!/usr/bin/env python3
"""Compare two existing host ttstt binaries; never build or deploy anything."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
import wave


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def cpu_set(value):
    cpus = set()
    try:
        for part in value.split(","):
            bounds = part.split("-")
            start = int(bounds[0])
            end = int(bounds[-1])
            if len(bounds) > 2 or start < 0 or end < start:
                raise ValueError
            cpus.update(range(start, end + 1))
        if not cpus:
            raise ValueError
    except ValueError:
        raise argparse.ArgumentTypeError("expected CPU IDs/ranges such as 0 or 0,2-3")
    return sorted(cpus)


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True, help="baseline host executable")
    parser.add_argument("--candidate", type=Path, required=True, help="candidate host executable")
    parser.add_argument("--output-dir", type=Path, required=True, help="fresh directory for logs, WAVs and results.json")
    parser.add_argument("--cpu", type=cpu_set, required=True, help="Linux CPU affinity, e.g. 0")
    parser.add_argument("--repeats", type=int, default=3, help="paired runs per case (default: 3)")
    parser.add_argument("--timeout", type=float, default=300, help="seconds per process (default: 300)")
    parser.add_argument("--corpus", type=Path, default=Path(__file__).with_name("perf_texts.json"))
    args = parser.parse_args()
    if args.repeats < 1 or args.timeout <= 0:
        parser.error("repeats and timeout must be positive")
    if not hasattr(os, "sched_setaffinity") or not set(args.cpu) <= os.sched_getaffinity(0):
        parser.error("requested CPU affinity is unavailable on this host")
    return args


def corpus(path):
    cases = json.loads(path.read_text(encoding="utf-8"))
    names = set()
    if not isinstance(cases, list) or not cases:
        raise ValueError("corpus must be a nonempty list of [case, text] pairs")
    for item in cases:
        if not isinstance(item, list) or len(item) != 2 or not all(isinstance(v, str) for v in item):
            raise ValueError("each corpus entry must be [case, text]")
        name, text = item
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or name in names or not text.strip():
            raise ValueError("case names must be unique filename-safe identifiers, with nonempty text")
        names.add(name)
    return cases


def run_binary(binary, variant, case, text, repeat, order, args, directory):
    stem = f"{case}-{repeat:02d}-{variant}"
    wav_path = directory / f"{stem}.wav"
    log_path = directory / f"{stem}.log"
    record = {
        "variant": variant, "case": case, "repeat": repeat, "order": order,
        "text_bytes": len(text.encode("utf-8")), "wav": str(wav_path), "log_path": str(log_path),
    }
    started = time.perf_counter()
    try:
        process = subprocess.run(
            [str(binary), "--text", text, "--wav", str(wav_path)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", timeout=args.timeout,
            preexec_fn=lambda: os.sched_setaffinity(0, args.cpu),
        )
        log = process.stdout
        record["returncode"] = process.returncode
        if process.returncode:
            record["error"] = f"process exited with {process.returncode}"
    except subprocess.TimeoutExpired as error:
        log = error.stdout or ""
        if isinstance(log, bytes):
            log = log.decode("utf-8", errors="replace")
        record.update(returncode=None, error=f"process exceeded {args.timeout:g} seconds")
    except OSError as error:
        log = ""
        record.update(returncode=None, error=str(error))
    record["wall_ms"] = round((time.perf_counter() - started) * 1000, 3)
    record["log"] = log
    log_path.write_text(log, encoding="utf-8")
    if "error" not in record:
        try:
            matches = re.findall(r"ttstt: job=\d+ done compute_ms=(\d+)\b", log)
            if len(matches) != 1:
                raise ValueError(f"expected one completed speech job, found {len(matches)}")
            record["compute_ms"] = int(matches[0])
            with wave.open(str(wav_path), "rb") as audio:
                frames, rate = audio.getnframes(), audio.getframerate()
                if not frames or not rate:
                    raise ValueError("empty WAV output")
                # Read the declared PCM payload so a truncated WAV cannot pass.
                expected_bytes = frames * audio.getnchannels() * audio.getsampwidth()
                if len(audio.readframes(frames)) != expected_bytes:
                    raise ValueError("truncated WAV output")
                record.update(
                    audio_ms=frames * 1000 / rate, frames=frames, sample_rate=rate,
                    channels=audio.getnchannels(), sample_width_bytes=audio.getsampwidth(),
                )
            record["wav_sha256"] = sha256(wav_path)
        except (OSError, ValueError, wave.Error, EOFError) as error:
            record["error"] = str(error)
    return record


def summarize(runs):
    summary = {}
    for case in dict.fromkeys(run["case"] for run in runs):
        result = {}
        for variant in ("baseline", "candidate"):
            selected = [run for run in runs if run["case"] == case and run["variant"] == variant and "error" not in run]
            if selected:
                result[variant] = {"runs": len(selected)}
                for metric in ("compute_ms", "wall_ms"):
                    values = [run[metric] for run in selected]
                    result[variant][f"{metric}_mean"] = statistics.mean(values)
                    result[variant][f"{metric}_median"] = statistics.median(values)
        if "baseline" in result and "candidate" in result:
            base = result["baseline"]["compute_ms_mean"]
            result["compute_change_percent"] = (result["candidate"]["compute_ms_mean"] / base - 1) * 100 if base else None
        summary[case] = result
    return summary


def main():
    args = arguments()
    binaries = {name: getattr(args, name).resolve(strict=True) for name in ("baseline", "candidate")}
    for binary in binaries.values():
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise ValueError(f"not an executable file: {binary}")
    cases = corpus(args.corpus)
    directory = args.output_dir.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    if any(directory.iterdir()):
        raise ValueError("output directory must be empty; previous evidence is never overwritten")
    result = {
        "schema": 1, "started_utc": datetime.now(timezone.utc).isoformat(),
        "cpu_affinity": args.cpu, "repeats": args.repeats,
        "corpus": {"path": str(args.corpus.resolve()), "sha256": sha256(args.corpus)},
        "binaries": {name: {"path": str(path), "sha256": sha256(path)} for name, path in binaries.items()},
        "runs": [], "pairs": [], "completed": False, "success": False,
    }
    (directory / "corpus.json").write_text(json.dumps(cases, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    def save():
        result["summary"] = summarize(result["runs"])
        pending = directory / "results.json.tmp"
        pending.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        pending.replace(directory / "results.json")

    save()
    try:
        for case, text in cases:
            for repeat in range(args.repeats):
                order = ("baseline", "candidate") if len(result["pairs"]) % 2 == 0 else ("candidate", "baseline")
                paired = {}
                for index, variant in enumerate(order):
                    run = run_binary(binaries[variant], variant, case, text, repeat, index, args, directory)
                    result["runs"].append(run)
                    paired[variant] = run
                    save()
                    if "error" in run:
                        raise ValueError(f"{case}/{repeat}/{variant}: {run['error']} (see {run['log_path']})")
                matches = paired["baseline"]["wav_sha256"] == paired["candidate"]["wav_sha256"]
                result["pairs"].append({"case": case, "repeat": repeat, "order": list(order), "wav_sha256_match": matches})
                save()
                if not matches:
                    raise ValueError(f"{case}/{repeat}: baseline and candidate WAV SHA-256 differ")
                print(f"{case} repeat={repeat}: baseline={paired['baseline']['compute_ms']} ms candidate={paired['candidate']['compute_ms']} ms WAV=exact", flush=True)
        for variant, binary in binaries.items():
            if sha256(binary) != result["binaries"][variant]["sha256"]:
                raise ValueError(f"{variant} binary changed during the experiment")
        result.update(completed=True, success=True)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    finally:
        result["finished_utc"] = datetime.now(timezone.utc).isoformat()
        save()
    print(f"Results: {directory / 'results.json'}")
    if not result["success"]:
        print(result["error"], file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"benchmark_cpu: {error}", file=sys.stderr)
        sys.exit(1)
