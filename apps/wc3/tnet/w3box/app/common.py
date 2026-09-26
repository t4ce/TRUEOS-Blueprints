"""W3Box shared configuration. Python standard library only."""
from __future__ import annotations

import dataclasses
import hashlib
import hmac
import ipaddress
import json
import os
import re
import secrets
import tempfile
from pathlib import Path
from typing import Any

PIN = "9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4"
DEFAULTS = {
    "server_name": "RoC 1.21b Realm",
    "server_address": "127.0.0.1",
    "bind_address": "0.0.0.0",
    "max_users": 64,
    "new_accounts": True,
    "strict_version": True,
    "advertise_ip": "",
}

@dataclasses.dataclass(frozen=True)
class Layout:
    root: Path = Path("/var/lib/w3box")
    prefix: Path = Path("/opt/w3box")
    runtime: Path = Path("/run/w3box")

    @property
    def conf(self): return self.root / "conf"
    @property
    def var(self): return self.root / "var"
    @property
    def binary(self): return self.prefix / "pvpgn/sbin/bnetd"
    @property
    def socket(self): return self.runtime / "control.sock"


def atomic_write(path: Path, data: str | bytes, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".w3box-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(data.encode("utf-8") if isinstance(data, str) else data)
            out.flush()
            os.fsync(out.fileno())
        os.chmod(tmp, mode)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp): os.unlink(tmp)


def read_json(path: Path, fallback: Any = None) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return fallback


def write_json(path: Path, data: Any) -> None:
    atomic_write(path, json.dumps(data, indent=2, sort_keys=True) + "\n")


def valid_host(host: str) -> bool:
    if not isinstance(host, str) or not 1 <= len(host) <= 253:
        return False
    try:
        ip = ipaddress.ip_address(host)
        return ip.version == 4 and not ip.is_unspecified and not ip.is_multicast
    except ValueError:
        if re.fullmatch(r"[0-9.]+", host): return False
        return all(re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", p)
                   for p in host.split("."))


def validate_settings(value: dict) -> dict:
    if not isinstance(value, dict) or set(value) != set(DEFAULTS):
        raise ValueError("Settings must contain exactly: " + ", ".join(DEFAULTS))
    s = value.copy()
    if not isinstance(s["server_name"], str) or not re.fullmatch(r"[A-Za-z0-9 ._-]{1,48}", s["server_name"]):
        raise ValueError("Server name: use 1–48 letters, numbers, spaces, dots, underscores or hyphens.")
    if not valid_host(s["server_address"]):
        raise ValueError("Server address must be an IPv4 address or DNS name, without a port or URL prefix.")
    try:
        ipaddress.IPv4Address(s["bind_address"])
    except (ValueError, TypeError):
        raise ValueError("Bind address must be an IPv4 address (0.0.0.0 means all interfaces).")
    if type(s["max_users"]) is not int or not 1 <= s["max_users"] <= 512:
        raise ValueError("Maximum users must be an integer from 1 to 512.")
    for key in ("new_accounts", "strict_version"):
        if type(s[key]) is not bool: raise ValueError(key + " must be true or false.")
    if not isinstance(s["advertise_ip"], str): raise ValueError("Advertise IP must be text.")
    if s["advertise_ip"]:
        try:
            ip = ipaddress.IPv4Address(s["advertise_ip"])
            if ip.is_unspecified or ip.is_multicast: raise ValueError()
        except (ValueError, TypeError):
            raise ValueError("Advertise IP must be an IPv4 address or empty for automatic routing.")
    return s


def load_settings(layout: Layout) -> dict:
    return validate_settings(read_json(layout.root / "settings.json", DEFAULTS.copy()))


def password_record(password: str) -> dict:
    if not isinstance(password, str) or not 12 <= len(password) <= 256:
        raise ValueError("Panel password must be 12–256 characters.")
    salt = secrets.token_bytes(24)
    iterations = 600_000
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, iterations)
    return {"salt": salt.hex(), "iterations": iterations, "hash": digest.hex()}


def check_password(password: str, record: dict) -> bool:
    if not isinstance(password, str) or len(password) > 256: return False
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), bytes.fromhex(record["salt"]), record["iterations"])
    return hmac.compare_digest(digest.hex(), record["hash"])


def set_directives(base: str, directives: dict[str, str]) -> str:
    """Replace active upstream directives; eliminate duplicate active assignments."""
    pending = directives.copy()
    lines = []
    seen = set()
    for line in base.splitlines():
        match = re.match(r"^\s*([A-Za-z0-9_]+)\s*=", line)
        key = match.group(1) if match else None
        if key in directives:
            if key not in seen:
                lines.append(f"{key} = {directives[key]}")
                seen.add(key)
                pending.pop(key, None)
        else:
            lines.append(line)
    lines.extend(f"{key} = {val}" for key, val in pending.items())
    return "\n".join(lines) + "\n"


def only_121b(data: dict) -> dict:
    """Keep the upstream equation and exact RoC 1.21b entry, not invented hashes."""
    output = {}
    for byte, spec in data["WAR3"]["IX86"].items():
        entries = [item for item in spec["entries"] if item.get("versionTag") == "WAR3_121B"]
        if entries:
            output[byte] = {**spec, "entries": entries}
    if not output:
        raise ValueError("Upstream source has no WAR3_121B entry; refusing an unverified configuration.")
    return {"WAR3": {"IX86": output}}


def render_config(layout: Layout, settings: dict) -> None:
    s = validate_settings(settings)
    base = (layout.prefix / "templates/bnetd.base.conf").read_text()
    overrides = {
        "servername": f'"{s["server_name"]}"',
        "description": f'"{s["server_name"]}"',
        "allowed_clients": "war3",
        "allow_bad_version": "false" if s["strict_version"] else "true",
        "allow_unknown_version": "false",
        "new_accounts": str(s["new_accounts"]).lower(),
        "max_concurrent_logins": str(s["max_users"]),
        "max_connections": str(max(128, s["max_users"] * 2)),
        "servaddrs": f'"{s["bind_address"]}:6112"',
        "w3routeaddr": f'"{s["bind_address"]}:6200"',
        "track": "0",
        "trackaddrs": '""',
        "loglevels": "fatal,error,warn,info",
        "shutdown_delay": "0",
        "shutdown_decr": "1",
        "sync_on_logoff": "true",
        "usersync": "60",
        "passfail_count": "5",
        "passfail_bantime": "300",
        "hide_addr": "true",
        "enable_conn_all": "false",
        "wolv1addrs": '""',
        "wolv2addrs": '""',
        "ircaddrs": '""',
        "telnetaddrs": '""',
        "pidfile": '""',
    }
    # The upstream template already contains absolute, CMake-generated paths.
    conf = set_directives(base, overrides)
    atomic_write(layout.conf / "bnetd.conf", conf, 0o640)
    versions = read_json(layout.prefix / "templates/versioncheck.upstream.json")
    atomic_write(layout.conf / "versioncheck.json", json.dumps(only_121b(versions), indent=2) + "\n", 0o640)
    # Do not supply patch upgrades from this private realm.
    atomic_write(layout.conf / "autoupdate.conf", "# W3Box: no automatic game upgrades.\n", 0o640)
    server_list = ("[Server List Version]\nVER=1001\n\n[Server Gateways]\n"
                   f"1={s['server_address']}\n\n[{s['server_address']}]\nZONE=0\n"
                   f"ENU={s['server_name']}\nDEU={s['server_name']}\n")
    atomic_write(layout.var / "files/bnserver-WAR3.ini", server_list, 0o640)
    atomic_write(layout.var / "files/bnserver.ini", server_list, 0o640)
    # Only manage our marked block; preserve manually added player/NAT translations.
    path = layout.conf / "address_translation.conf"
    current = path.read_text() if path.exists() else "# PvPGN address translation\n"
    current = re.sub(r"\n?# W3BOX ROUTE BEGIN\n.*?# W3BOX ROUTE END\n?", "\n", current, flags=re.S)
    block = "\n# W3BOX ROUTE BEGIN\n"
    if s["advertise_ip"]:
        block += f"{s['bind_address']}:6200 {s['advertise_ip']}:6200 NONE ANY\n"
    block += "# W3BOX ROUTE END\n"
    atomic_write(path, current.rstrip() + "\n" + block, 0o640)


def initialize(layout: Layout, address: str, password: str | None = None) -> str | None:
    """Idempotent initialization: preserve existing settings and password."""
    for path in (layout.root, layout.conf, layout.var, layout.root / "backups"):
        path.mkdir(parents=True, exist_ok=True)
    if not (layout.root / "settings.json").exists():
        settings = DEFAULTS.copy()
        settings["server_address"] = address
        try:
            settings["advertise_ip"] = str(ipaddress.IPv4Address(address))
        except ValueError:
            pass
        write_json(layout.root / "settings.json", validate_settings(settings))
    generated = None
    if not (layout.root / "auth.json").exists():
        generated = password or secrets.token_urlsafe(21)
        write_json(layout.root / "auth.json", password_record(generated))
    if not (layout.root / "state.json").exists():
        write_json(layout.root / "state.json", {"desired_running": True})
    render_config(layout, load_settings(layout))
    return generated
