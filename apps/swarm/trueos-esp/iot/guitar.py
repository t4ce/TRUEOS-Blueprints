"""SWARM-uploadable MicroPython guitar app; existing firmware supplies Wi-Fi.

ASCII TCP :24816: BAR <bpm> <frets> <frets> <frets> <frets>\n
Each fret word has one 0..4 digit per string, high to low; --- is a rest.
One fixed high-to-low sweep per non-rest beat. STOP releases all frets.
"""
import socket
import time
from machine import Pin, PWM

# ESP32-S3 defaults, deliberately >= 10. Change for your actual board/wiring.
# Rows: high E, B, G. Columns: fret 1, 2, 3, 4. Zero releases the row.
FRET_GPIOS = ((10, 11, 12, 13), (14, 15, 16, 17), (18, 21, 38, 39))
SERVO_GPIO = 40
RELAY_ACTIVE = 1  # Set to 0 for active-low relay inputs.
SERVO_HZ = 50
SERVO_MIN_US = 500
SERVO_MAX_US = 2500
SERVO_HOME_DEG = 0
SERVO_END_DEG = 270
FRET_SETTLE_MS = 40
SWEEP_MS = 120
RETURN_MS = 120
START_DELAY_MS = 250
MAX_LATE_MS = 30
MIN_BPM = 30
MAX_BPM = 120
PORT = 24816
MAX_LINE = 256


def parse_bar(line):
    words = line.split()
    if len(words) != 6 or words[0] != "BAR":
        raise ValueError("expected BAR bpm fret fret fret fret")
    bpm = int(words[1])
    if not MIN_BPM <= bpm <= MAX_BPM:
        raise ValueError("bpm outside configured range")
    beat_ms = 60000 // bpm
    if beat_ms <= FRET_SETTLE_MS + SWEEP_MS + RETURN_MS + 2 * MAX_LATE_MS:
        raise ValueError("beat too short for configured mechanism")
    frames = []
    for word in words[2:]:
        if word == "-" * len(FRET_GPIOS):
            frames.append(None)
        elif len(word) == len(FRET_GPIOS) and all(c in "01234" for c in word):
            frames.append(tuple(int(c) for c in word))
        else:
            raise ValueError("frets must be 0..4 per string or all dashes")
    return beat_ms, frames


class Guitar:
    def __init__(self):
        self.rows = []
        self.servo = None
        self.events = []
        gpios = [pin for row in FRET_GPIOS for pin in row] + [SERVO_GPIO]
        if any(len(row) != 4 for row in FRET_GPIOS) or len(set(gpios)) != len(gpios):
            raise ValueError("four unique fret pins per string and a separate servo pin required")
        try:
            for row in FRET_GPIOS:
                pins = []
                self.rows.append(pins)
                for gpio in row:
                    pins.append(Pin(gpio, Pin.OUT, value=1 - RELAY_ACTIVE))
            self.servo = PWM(Pin(SERVO_GPIO), freq=SERVO_HZ)
            self.stop()
        except Exception:
            self.close()
            raise

    def angle(self, degrees):
        pulse = SERVO_MIN_US + degrees * (SERVO_MAX_US - SERVO_MIN_US) // 270
        self.servo.duty_ns(pulse * 1000)

    def frets(self, values):
        # Break before make, so two frets on a string never remain energized.
        for row in self.rows:
            for pin in row:
                pin.value(1 - RELAY_ACTIVE)
        for row, fret in zip(self.rows, values):
            if fret:
                row[fret - 1].value(RELAY_ACTIVE)

    def stop(self):
        self.events = []
        self.frets([0] * len(self.rows))
        if self.servo is not None:
            self.angle(SERVO_HOME_DEG)

    def close(self):
        self.stop()
        if self.servo is not None:
            self.servo.deinit()

    def load(self, line, now):
        beat_ms, frames = parse_bar(line)
        if self.events:
            raise ValueError("busy")
        events = []
        for beat, frame in enumerate(frames):
            offset = START_DELAY_MS + beat * beat_ms
            events.append((time.ticks_add(now, offset - FRET_SETTLE_MS - MAX_LATE_MS), "frets", frame))
            if frame is not None:
                events.append((time.ticks_add(now, offset), "sweep", None))
                events.append((time.ticks_add(now, offset + SWEEP_MS), "home", None))
        events.append((time.ticks_add(now, START_DELAY_MS + 4 * beat_ms), "done", None))
        self.events = events

    def tick(self, now):
        if not self.events or time.ticks_diff(now, self.events[0][0]) < 0:
            return None
        deadline, action, value = self.events.pop(0)
        if time.ticks_diff(now, deadline) > MAX_LATE_MS:
            self.stop()
            return "ERR late; stopped\n"
        if action == "frets":
            self.frets(value if value is not None else [0] * len(self.rows))
        elif action == "sweep":
            self.angle(SERVO_END_DEG)
        elif action == "home":
            self.angle(SERVO_HOME_DEG)
        elif action == "done":
            self.stop()
            return "DONE\n"
        return None


def serve_client(client, guitar):
    client.settimeout(0.005)
    pending = b""
    client.sendall(b"READY GUITAR 1\n")
    while True:
        reply = guitar.tick(time.ticks_ms())
        if reply:
            client.sendall(reply.encode("ascii"))
        try:
            data = client.recv(128)
        except OSError as error:
            if error.args[0] in (11, 110, 116):  # EAGAIN / ETIMEDOUT
                continue
            raise
        if not data:
            return
        pending += data
        if len(pending) > MAX_LINE:
            guitar.stop()
            client.sendall(b"ERR line too long; stopped\n")
            return
        while b"\n" in pending:
            line, pending = pending.split(b"\n", 1)
            try:
                line = line.decode("ascii").strip()
                if line == "STOP":
                    guitar.stop()
                    reply = "OK STOP\n"
                elif line == "STATUS":
                    reply = "BUSY\n" if guitar.events else "IDLE\n"
                else:
                    guitar.load(line, time.ticks_ms())
                    reply = "OK BAR\n"
            except (ValueError, UnicodeError) as error:
                reply = "ERR %s\n" % error
            client.sendall(reply.encode("ascii"))


def main():
    guitar = Guitar()
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(("0.0.0.0", PORT))
        server.listen(1)
        print("Guitar ASCII TCP listening on", PORT)
        while True:
            client, address = server.accept()
            try:
                serve_client(client, guitar)
            except OSError as error:
                print("Guitar connection:", error)
            finally:
                guitar.stop()
                client.close()
    finally:
        guitar.close()
        server.close()


if __name__ == "__main__":
    main()
