#!/usr/bin/env python3
"""Record physical keys against this GLB's keycaps. Linux, Python stdlib + libxkbcommon."""
import argparse
import ast
import ctypes as C
import ctypes.util
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import select
import struct
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
MODEL = HERE.parent / "Keyboard k70 mini pro.glb"
EVENT = struct.Struct("@llHHi")  # Linux input_event, native timeval size/alignment
EVIOCGRAB = 0x40044590
SPACE = 57
# Physical rows, left to right. The tall ISO Enter belongs to the Tab/Q row.
ROWS = [
    list(range(14)),
    [17, 14, *range(30, 41), 28],
    [18, 15, *range(41, 52)],
    [19, 16, *range(52, 63)],
    list(range(20, 28)),
]
SPECIAL = {
    1: "Esc", 14: "Backspace", 15: "Tab", 28: "Enter", 29: "Ctrl",
    42: "Shift", 54: "Shift", 56: "Alt", 57: "Space", 58: "Caps",
    97: "Ctrl", 100: "AltGr", 125: "Super", 126: "Super", 127: "Menu",
}


def model_slots():
    blob = MODEL.read_bytes()
    magic, version, length, chunk_len, chunk_type = struct.unpack_from("<5I", blob)
    if (magic, version, length, chunk_type) != (0x46546C67, 2, len(blob), 0x4E4F534A):
        raise ValueError("Invalid keyboard GLB")
    doc = json.loads(blob[20:20 + chunk_len])
    if len(doc["meshes"]) != 63 or len(doc["nodes"]) != 63:
        raise ValueError("Keyboard model changed: review the key order first")
    slots = []
    for row, nodes in enumerate(ROWS, 1):
        for column, node_id in enumerate(nodes, 1):
            node = doc["nodes"][node_id]
            if node.get("mesh") != node_id or node.get("children"):
                raise ValueError("Keyboard node/mesh arrangement changed")
            slots.append(dict(slot=len(slots) + 1, row=row, column=column,
                              node=node_id, mesh=node["mesh"], name=node.get("name"),
                              origin=node["translation"], spacebar=node_id == 23))
    if len({slot["mesh"] for slot in slots}) != 62:
        raise ValueError("Key order contains duplicate or missing meshes")
    return hashlib.sha256(blob).hexdigest(), slots


def detect_layout():
    # Run as the desktop user for detection; the suggested sudo command below
    # carries this result explicitly, rather than reading root's preferences.
    try:
        result = subprocess.run(
            ["gsettings", "get", "org.gnome.desktop.input-sources", "sources"],
            capture_output=True, text=True, timeout=3, check=True)
        for kind, source in ast.literal_eval(result.stdout):
            if kind == "xkb":
                parts = source.split("+", 1)
                return parts[0], parts[1] if len(parts) == 2 else ""
    except (OSError, ValueError, SyntaxError, subprocess.SubprocessError):
        pass
    return None, ""


class Xkb:
    """Read unmodified/Shift/AltGr legends without changing desktop lock state."""
    def __init__(self, layout, variant):
        self.lib = C.CDLL(ctypes.util.find_library("xkbcommon") or "libxkbcommon.so.0")
        class Names(C.Structure):
            _fields_ = [(key, C.c_char_p) for key in
                        ("rules", "model", "layout", "variant", "options")]
        signatures = {
            "context_new": (C.c_void_p, [C.c_int]),
            "keymap_new_from_names": (C.c_void_p, [C.c_void_p, C.POINTER(Names), C.c_int]),
            "keymap_num_levels_for_key": (C.c_uint, [C.c_void_p, C.c_uint, C.c_uint]),
            "keymap_key_get_syms_by_level": (C.c_int, [C.c_void_p, C.c_uint, C.c_uint,
                                                       C.c_uint, C.POINTER(C.POINTER(C.c_uint))]),
            "keysym_get_name": (C.c_int, [C.c_uint, C.c_char_p, C.c_size_t]),
            "keysym_to_utf32": (C.c_uint, [C.c_uint]),
            "keymap_unref": (None, [C.c_void_p]),
            "context_unref": (None, [C.c_void_p]),
        }
        for name, (result, args) in signatures.items():
            fn = getattr(self.lib, "xkb_" + name)
            fn.restype, fn.argtypes = result, args
            setattr(self, name, fn)
        self.context = self.context_new(0)
        names = Names(b"evdev", b"pc105", layout.encode(), variant.encode(), b"")
        self.keymap = self.keymap_new_from_names(self.context, C.byref(names), 0)
        if not self.keymap:
            self.context_unref(self.context)
            raise ValueError("Cannot load XKB layout/variant")

    def legends(self, code):
        legends = []
        key = code + 8  # Linux evdev -> XKB keycode
        for level in range(self.keymap_num_levels_for_key(self.keymap, key, 0)):
            symbols = C.POINTER(C.c_uint)()
            count = self.keymap_key_get_syms_by_level(self.keymap, key, 0, level,
                                                     C.byref(symbols))
            for symbol in symbols[:count]:
                name = C.create_string_buffer(128)
                self.keysym_get_name(symbol, name, len(name))
                value = self.keysym_to_utf32(symbol)
                text = chr(value) if value and chr(value).isprintable() else ""
                legends.append(dict(level=level, keysym=name.value.decode(), text=text))
        return legends

    def close(self):
        self.keymap_unref(self.keymap)
        self.context_unref(self.context)


def linux_key_names():
    path = Path("/usr/include/linux/input-event-codes.h")
    if not path.exists():
        return {}
    return {int(value, 0): name for name, value in re.findall(
        r"^#define\s+(KEY_\w+)\s+(0x[0-9a-fA-F]+|\d+)\b", path.read_text(), re.M)}


def keyboard_devices():
    devices = []
    for event in sorted(Path("/sys/class/input").glob("event*")):
        try:
            # Require ordinary alphanumeric keys, excluding mouse/media interfaces.
            words = (event / "device/capabilities/key").read_text().split()
            bits = int("".join(word.zfill(C.sizeof(C.c_ulong) * 2) for word in words), 16)
            if all(bits & (1 << code) for code in (1, 16, 30, SPACE)):
                devices.append((Path("/dev/input") / event.name,
                                (event / "device/name").read_text().strip()))
        except (OSError, ValueError):
            continue
    return devices


def atomic_write(path, text):
    temporary = path.with_name(path.name + ".tmp")
    try:
        with temporary.open("w", encoding="utf-8") as stream:
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        # Keep captures editable by the invoking user when raw input required sudo.
        if os.geteuid() == 0 and "SUDO_UID" in os.environ:
            os.chown(temporary, int(os.environ["SUDO_UID"]), int(os.environ["SUDO_GID"]))
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def save(path, data):
    atomic_write(path, json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    lines = ["slot\trow\tcolumn\tmesh\tstatus\tlinux_key\tscan\tlabel\tlegends"]
    for slot in data["keys"]:
        press = slot.get("press", {})
        legends = " / ".join(item["text"] or item["keysym"]
                             for item in press.get("legends", []))
        lines.append("\t".join(str(value) for value in (
            slot["slot"], slot["row"], slot["column"], slot["mesh"],
            press.get("status", "pending"), press.get("linux_key", ""),
            press.get("scan", ""), press.get("label", ""), legends)))
    atomic_write(path.with_suffix(".txt"), "\n".join(lines) + "\n")


def load(path, model_hash, slots, layout, variant):
    if path.exists():
        data = json.loads(path.read_text())
        if data.get("schema") != 1 or data.get("model_sha256") != model_hash:
            raise ValueError("Saved mapping belongs to a different model/schema")
        if (data["layout"], data["variant"]) != (layout, variant):
            raise ValueError("Use the original --layout/--variant when resuming")
        if len(data["keys"]) != len(slots) or any(
                {k: v for k, v in old.items() if k != "press"} != new
                for old, new in zip(data["keys"], slots)):
            raise ValueError("Saved mapping key order differs from the model")
        seen_pending = False
        for slot in data["keys"]:
            if "press" not in slot:
                seen_pending = True
            elif seen_pending:
                raise ValueError("Saved mapping contains a hole; fix before resuming")
        return data
    return dict(schema=1, model=MODEL.name, model_sha256=model_hash,
                layout=layout, variant=variant, keys=slots)


def next_slot(data):
    return next((slot for slot in data["keys"] if "press" not in slot), None)


def prompt(data):
    slot = next_slot(data)
    if slot:
        hint = " — actual SPACEBAR" if slot["spacebar"] else ""
        if slot["mesh"] == 28:
            hint = " — tall ENTER (record once here, not on the next row)"
        print(f'Next {slot["slot"]}/62: row {slot["row"]}, key {slot["column"]}'
              f' from left, mesh {slot["mesh"]}{hint}', flush=True)


def record_press(data, code, scan, xkb, names):
    slot = next_slot(data)
    if slot is None:
        return
    if code == SPACE and not slot["spacebar"]:
        press = dict(status="skipped", label="", reason="space-joker")
    else:
        legends = xkb.legends(code)
        label = SPECIAL.get(code) or next((item["text"] or item["keysym"]
                                          for item in legends), names.get(code, f"key-{code}"))
        press = dict(status="mapped", linux_code=code,
                     linux_key=names.get(code, f"KEY_{code}"), scan=scan,
                     label=label, legends=legends)
    slot["press"] = press
    print(f'  {slot["slot"]}: {press["label"] or "SKIP"}', flush=True)


def capture(fd, data, path, xkb):
    names = linux_key_names()
    pending_scan = None
    escape_at = None
    escape_scan = None
    down = set()
    prompt(data)
    while next_slot(data):
        if escape_at is not None and time.monotonic() - escape_at >= 2:
            print("\nPaused. Release Esc; run again to resume.", flush=True)
            return
        if not select.select([fd], [], [], 0.1)[0]:
            continue
        packet = os.read(fd, EVENT.size * 64)
        if not packet or len(packet) % EVENT.size:
            raise OSError("Keyboard disconnected or returned a partial event")
        for _, _, kind, code, value in EVENT.iter_unpack(packet):
            if kind == 0 and code == 3:  # SYN_DROPPED: never shift slots silently.
                raise OSError("Input queue overflow; paused, check the last recorded key")
            if kind == 0 and code == 0:
                pending_scan = None
            elif kind == 4 and code == 4:  # MSC_SCAN, commonly page-qualified USB HID usage
                pending_scan = value & 0xffffffff
            elif kind == 1:
                if value == 0:
                    down.discard(code)
                    if code == 1 and escape_at is not None:
                        record_press(data, code, escape_scan, xkb, names)
                        escape_at = None
                        save(path, data)
                        prompt(data)
                elif value == 1 and code not in down:
                    down.add(code)
                    if code == 1:
                        # A tap maps Esc; a hold pauses without consuming a slot.
                        escape_at, escape_scan = time.monotonic(), pending_scan
                    elif escape_at is None:
                        record_press(data, code, pending_scan, xkb, names)
                        save(path, data)
                        prompt(data)
                # value=2 is repeat: never advance the mapping.
        if next_slot(data) is None:
            # Finish the packet first: it may already contain the final key-up.
            deadline = time.monotonic() + 2
            while down and time.monotonic() < deadline:
                if select.select([fd], [], [], 0.1)[0]:
                    for *_, k, c, v in EVENT.iter_unpack(os.read(fd, EVENT.size * 64)):
                        if k == 1 and v == 0:
                            down.discard(c)
            print("All 62 positions recorded.", flush=True)
            return


def main():
    detected_layout, detected_variant = detect_layout()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", type=Path, help="/dev/input/eventN or by-id event-kbd")
    parser.add_argument("--layout", default=detected_layout, help="XKB layout, e.g. de or us")
    parser.add_argument("--variant", default=detected_variant, help="XKB variant (default: detected)")
    parser.add_argument("--output", type=Path, default=HERE / "mapping.json")
    parser.add_argument("--list", action="store_true", help="list raw keyboard devices and exit")
    parser.add_argument("--plan", action="store_true", help="print model order, no recording")
    parser.add_argument("--undo", type=int, default=0, metavar="N",
                        help="remove last N entries and exit; next run resumes there")
    args = parser.parse_args()
    if args.list:
        for path, name in keyboard_devices():
            print(f"{path}: {name}")
        return
    model_hash, slots = model_slots()
    if args.plan:
        for slot in slots:
            print(f'{slot["slot"]:2}  row {slot["row"]} key {slot["column"]:2}'
                  f'  mesh {slot["mesh"]:2}  {"SPACEBAR" if slot["spacebar"] else ""}')
        return
    if not args.layout:
        parser.error("Cannot detect desktop layout; supply --layout de (or your XKB layout)")
    path = args.output.resolve()
    if not path.parent.is_dir():
        parser.error("Output directory must already exist")
    # Lock before loading, including undo, so no recorder can overwrite a newer map.
    with path.with_suffix(".lock").open("a") as lock:
        if os.geteuid() == 0 and "SUDO_UID" in os.environ:
            os.fchown(lock.fileno(), int(os.environ["SUDO_UID"]), int(os.environ["SUDO_GID"]))
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        run(args, parser, path, model_hash, slots)


def run(args, parser, path, model_hash, slots):
    data = load(path, model_hash, slots, args.layout, args.variant)
    if args.undo:
        mapped = [slot for slot in data["keys"] if "press" in slot]
        if not 0 < args.undo <= len(mapped):
            parser.error("--undo must be between 1 and the number of recorded positions")
        for slot in mapped[-args.undo:]:
            del slot["press"]
        save(path, data)
        prompt(data)
        return
    if not next_slot(data):
        print(f"Mapping already complete: {path}")
        return
    device = args.device
    if device is None:
        devices = keyboard_devices()
        if len(devices) != 1:
            parser.error("Choose a keyboard with --device; use --list to see candidates")
        device = devices[0][0]
    print(f"Device: {device}\nLegends: XKB {args.layout} {args.variant}\nOutput: {path}")
    try:
        fd = os.open(device, os.O_RDONLY | os.O_NONBLOCK)
    except PermissionError:
        import shlex
        command = ["sudo", sys.executable, str(Path(__file__).resolve()),
                   "--device", str(device), "--layout", args.layout,
                   "--variant", args.variant, "--output", str(path)]
        parser.exit(1, "Raw keyboard access needs permission. Run:\n" + shlex.join(command) + "\n")
    xkb = None
    grabbed = False
    try:
        xkb = Xkb(args.layout, args.variant)
        print("Release all keys. Starting in 3 seconds.\n"
              "Tap each key left to right, top to bottom, beginning at ESC.\n"
              "Space = skip, except at the spacebar. Hold ESC for 2 seconds to pause.\n"
              "This keyboard is captured exclusively until completion/pause.", flush=True)
        time.sleep(3)
        # Drain the Enter used to launch the command, before taking ownership.
        while select.select([fd], [], [], 0)[0]:
            os.read(fd, EVENT.size * 64)
        fcntl.ioctl(fd, EVIOCGRAB, 1)
        grabbed = True
        save(path, data)
        capture(fd, data, path, xkb)
    finally:
        if grabbed:
            fcntl.ioctl(fd, EVIOCGRAB, 0)
        os.close(fd)
        if xkb:
            xkb.close()
    print(f"Saved: {path}\nReadable copy: {path.with_suffix('.txt')}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nPaused; the last saved mapping will resume next run.")
    except (OSError, ValueError) as error:
        sys.exit(f"keyfarm: {error}")
