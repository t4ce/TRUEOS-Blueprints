#!/usr/bin/env python3
"""Send one locally timed 4/4 bar to the SWARM guitar app."""
import argparse
import socket


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", help="discovered ESP IPv4 address")
    parser.add_argument("frets", nargs="*", help="four high-to-low fret words, e.g. 000 100 300 rest")
    parser.add_argument("--bpm", type=int, default=60)
    parser.add_argument("--port", type=int, default=24816)
    parser.add_argument("--stop", action="store_true")
    args = parser.parse_intermixed_args()
    args.frets = ["---" if word == "rest" else word for word in args.frets]
    if not args.stop:
        if len(args.frets) != 4 or not 30 <= args.bpm <= 120:
            parser.error("provide four fret words and a tempo from 30 to 120 BPM")
        if any(word != "---" and (len(word) != 3 or any(c not in "01234" for c in word))
               for word in args.frets):
            parser.error("each word must have three digits 0..4, or be rest")
    with socket.create_connection((args.host, args.port), timeout=5) as connection:
        connection.settimeout(15)
        with connection.makefile("rb") as reader:
            def receive():
                line = reader.readline(256)
                if not line:
                    raise RuntimeError("board disconnected")
                reply = line.decode("ascii").strip()
                print(reply)
                if reply.startswith("ERR"):
                    raise RuntimeError(reply)
                return reply

            if receive() != "READY GUITAR 1":
                raise RuntimeError("unexpected guitar greeting")
            command = "STOP" if args.stop else "BAR %d %s" % (args.bpm, " ".join(args.frets))
            connection.sendall((command + "\n").encode("ascii"))
            receive()
            if not args.stop and receive() != "DONE":
                raise RuntimeError("unexpected completion response")


if __name__ == "__main__":
    main()
