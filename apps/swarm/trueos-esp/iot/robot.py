import socket
import json
from machine import Pin, PWM

UDP_IP = "192.168.178.118"
UDP_PORT = 24816
BUFFER_SIZE = 4096

# Mot0 -> GPIO1
# Mot1 -> GPIO2
servos = [
    PWM(Pin(1), freq=50),
    PWM(Pin(2), freq=50),
]


def set_servo_angle(servo, angle):
    angle = max(0, min(270, int(angle)))

    pulse_us = 500 + (angle * 2000 // 270)

    servo.duty_ns(pulse_us * 1000)


def handle_motors_packet(payload):
    try:
        data = json.loads(payload)
    except Exception:
        return

    if not isinstance(data, dict):
        return

    for index, servo in enumerate(servos):
        key = f"Mot{index}"

        if key in data:
            angle = data[key]
            set_servo_angle(servo, angle)

            print(
                key,
                "angle:",
                angle,
                "pulse:",
                500 + int(angle) * 2000 // 270,
                "us",
            )


def main():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))

    print("Listening on", UDP_IP, UDP_PORT)

    while True:
        payload, address = sock.recvfrom(BUFFER_SIZE)

        try:
            text = payload.decode("utf-8")
        except UnicodeDecodeError:
            continue

        print("Packet:", text)

        handle_motors_packet(text)


if __name__ == "__main__":
    main()