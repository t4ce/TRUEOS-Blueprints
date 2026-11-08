# Guitar output app

`iot/guitar.py` is an on-demand MicroPython app for the existing ESP uploader.
SWARM embeds it as the **guitar** sketch: select a discovered ESP, open the
template, adjust the constants, then **Upload as app.py & run**. The existing
firmware supplies Wi-Fi, discovery, upload, and execution. This app adds only an
ASCII TCP listener on port 24816; it does not start another discovery service.

The equivalent upload from this directory is:

```sh
DEVICE_HOST=192.168.178.102 bash ./uploadexample ./iot/guitar.py
```

## Mechanism and pin constants

The initial mechanism has **three strings**, four independent fret solenoids
per string, and one digital position servo that sweeps all three from high to
low. Each fret uses a GPIO-controlled relay input. Open string means all four
relays in that row are off; selecting a fret energizes only that relay. Relays
are held for the note, rather than pulsed at 100 Hz. GPIOs drive the relay
module's logic inputs, not relay coils or solenoids directly.

The configurable defaults target an **ESP32-S3**, using GPIO10 and above:

| String order | Fret 1 | Fret 2 | Fret 3 | Fret 4 |
| --- | --- | --- | --- | --- |
| High E | 10 | 11 | 12 | 13 |
| B | 14 | 15 | 16 | 17 |
| G | 18 | 21 | 38 | 39 |

Servo signal: GPIO40. `RELAY_ACTIVE` controls active-high/active-low modules.
Adjust `FRET_GPIOS`, `SERVO_GPIO`, and the servo pulse/angle constants to match
the actual board and mechanism before running it. The S3 defaults avoid USB
GPIO19/20 and common flash/PSRAM pins; verify board-specific assignments.
The C3 has no GPIO above 21 and usually reserves GPIO12–17 for flash, so this
13-output arrangement using only GPIO10 and higher needs an S3 or an output
expander. No expander driver is included.

Servo commands map 0–270 degrees to configurable 500–2500 microsecond pulses at
50 Hz. Those are initial calibration values, not measured limits of your servo.
One sweep uses a fixed endpoint and fixed timing; there is no force control.
The return travels back over the same angular path. The pick mechanism must
clear or yield to the strings on return, otherwise it will strum in reverse too.

Default tuning, high to low, with frets 0–4:

| String | Open | 1 | 2 | 3 | 4 |
| --- | --- | --- | --- | --- | --- |
| E4 | E4 | F4 | F#4 | G4 | G#4 |
| B3 | B3 | C4 | C#4 | D4 | D#4 |
| G3 | G3 | G#3 | A3 | A#3 | B3 |

The hardware always strums all three. A fret word therefore selects a
three-note voicing; it cannot request an isolated string pluck. Releasing a fret
does not mute the string. Rests omit the sweep and release frets but cannot stop
existing sound. There is no tuning feedback or audio input.

## ASCII control

Connect to the device on TCP port 24816. The board greets with
`READY GUITAR 1`. Commands and replies are ASCII lines terminated by LF:

```text
BAR 60 000 100 300 ---
```

The board validates and buffers a complete 4/4 bar, replies `OK BAR`, and plays
one word per quarter-note beat at 60 BPM. Digits are **high-to-low string order**,
each from `0` (open) to `4` (fourth fret). `---` is a rest. Each non-rest word
triggers one complete high-to-low servo sweep. After four beats, all frets
release and the board replies `DONE`. Another bar can then be sent on the same
connection; this first version has a preparation gap between bars.

```sh
python3 guitar_send.py 192.168.178.102 --bpm 60 000 100 300 rest
```

`STATUS` replies `BUSY` or `IDLE`. `STOP` cancels the bar, releases the frets,
commands the servo home, and replies `OK STOP`. A disconnect also cancels the
bar. There is one controlling connection at a time; send STOP on that connection
or close it. `guitar_send.py --stop HOST` is useful when idle; it cannot preempt
another connected client. Interrupting the sender closes its connection.

Invalid commands reply `ERR ...`; a second BAR while playing replies `ERR busy`.
An overlong input or execution delay beyond `MAX_LATE_MS` cancels playback.
TCP handles packet loss/order; scheduling uses the MCU's wrap-safe local ticks,
not packet arrival for each beat. The initial start delay is 250 ms, frets
prepare at least 40 ms ahead, and the servo gets 120 ms outward plus 120 ms
return. Tempo is limited to 30–120 BPM and checked against configured movement
times. These are configurable budgets, not measured guarantees. There is no
position feedback, and successful command execution cannot detect a lagging
motor. Frets release after at most the buffered bar if the sender disappears
without a detectable disconnect.

Hardware/API references: [Espressif C3 GPIO restrictions](https://docs.espressif.com/projects/esp-idf/en/v5.0.4/esp32c3/api-reference/peripherals/gpio.html),
[Espressif S3 GPIO restrictions](https://docs.espressif.com/projects/esp-idf/en/stable/esp32s3/api-reference/peripherals/gpio.html),
and [MicroPython ESP32 PWM/network API](https://docs.micropython.org/en/latest/esp32/quickref.html).
