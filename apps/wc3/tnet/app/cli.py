#!/usr/bin/env python3
"""Command-line controls for the W3Box service (normally run with sudo)."""
from __future__ import annotations
import argparse
import getpass
import json
import socket
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import DEFAULTS


def request(command, data=None, path="/run/w3box/control.sock"):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
        conn.settimeout(65)
        conn.connect(path)
        conn.sendall(json.dumps({"command": command, "data": data or {}}).encode() + b"\n")
        with conn.makefile("rb") as stream:
            reply = json.loads(stream.readline(1_000_000))
    if not reply.get("ok"): raise RuntimeError(reply.get("error", "Operation failed."))
    return reply["result"]


def main():
    p = argparse.ArgumentParser(description=__doc__, epilog="Examples: sudo w3ctl status ; sudo w3ctl set new_accounts false ; sudo w3ctl logs")
    p.add_argument("command", choices=["status", "start", "stop", "restart", "logs", "doctor", "backup", "config", "set", "gateway", "password"])
    p.add_argument("arguments", nargs="*")
    p.add_argument("--json", action="store_true", help="Machine-readable result")
    p.add_argument("--socket", default="/run/w3box/control.sock", help=argparse.SUPPRESS)
    args = p.parse_args()
    command, data = args.command, {}
    if command == "set":
        if len(args.arguments) != 2: p.error("set requires KEY VALUE")
        key, value = args.arguments
        if key not in DEFAULTS: p.error("Unknown setting. Keys: " + ", ".join(DEFAULTS))
        data = request("settings", path=args.socket)
        if type(DEFAULTS[key]) is bool:
            if value.lower() not in ("true", "false"): p.error("Use true or false.")
            value = value.lower() == "true"
        elif type(DEFAULTS[key]) is int:
            try: value = int(value)
            except ValueError: p.error("An integer is required.")
        data[key] = value
        command = "settings"
    elif args.arguments:
        p.error("This command does not accept additional arguments.")
    if command == "password":
        password = getpass.getpass("New panel password (12+ characters): ")
        if password != getpass.getpass("Repeat password: "): p.error("Passwords do not match.")
        data = {"password": password}
    if command == "config": command = "settings"
    result = request(command, data, args.socket)
    if args.json:
        print(json.dumps(result, indent=2))
    elif command == "logs":
        print("=== PvPGN server log ===\n" + result["server"] + "\n\n=== Process startup output ===\n" + result["startup"])
    elif command == "gateway":
        print(result["script"], end="")
    elif command == "doctor":
        for check in result["checks"]:
            print(f"{'OK  ' if check['ok'] else 'FAIL'} {check['name']}: {check['detail']}")
        print("\n" + "\n".join(result["notes"]))
    elif "process_running" in result:
        print(f"{result['server_name']} | {'READY' if result['ready'] else 'STARTING / NOT READY' if result['process_running'] else 'STOPPED'}")
        print(f"Address: {result['server_address']}:6112 | PID: {result['pid']} | Uptime: {result['uptime_seconds']} seconds")
        print(f"Account files: {result['account_files']} | Auto-start requested: {result['desired_running']}")
        if result["last_error"]: print("Last error: " + result["last_error"])
    else:
        print(json.dumps(result, indent=2))
    if command == "doctor" and not all(c["ok"] for c in result["checks"]): return 2
    return 0

if __name__ == "__main__":
    try: sys.exit(main())
    except (OSError, ValueError, RuntimeError) as exc:
        print(f"w3ctl: {exc}\nCheck: sudo systemctl status w3box --no-pager", file=sys.stderr)
        sys.exit(1)
