#![no_std]

use trueos::logl::{self, level};

const MOTOR_MIN_US: f32 = 1000.0;
const MOTOR_MAX_US: f32 = 2000.0;
const BASE_THROTTLE_US: f32 = 1320.0;
const MAX_CORRECTION_US: f32 = 190.0;

#[derive(Clone, Copy)]
struct Attitude {
    roll_deg: f32,
    pitch_deg: f32,
    roll_rate_dps: f32,
    pitch_rate_dps: f32,
}

#[derive(Clone, Copy)]
struct Receiver {
    throttle_us: f32,
    roll_deg: f32,
    pitch_deg: f32,
}

#[derive(Clone, Copy)]
struct Motors {
    front_left: f32,
    front_right: f32,
    rear_left: f32,
    rear_right: f32,
}

struct Pid {
    kp: f32,
    ki: f32,
    kd: f32,
    integral: f32,
    previous_error: f32,
}

impl Pid {
    const fn new(kp: f32, ki: f32, kd: f32) -> Self {
        Self {
            kp,
            ki,
            kd,
            integral: 0.0,
            previous_error: 0.0,
        }
    }

    fn update(&mut self, error: f32, dt: f32) -> f32 {
        self.integral = clamp(self.integral + error * dt, -80.0, 80.0);
        let derivative = if dt > 0.0 {
            (error - self.previous_error) / dt
        } else {
            0.0
        };
        self.previous_error = error;
        clamp(
            self.kp * error + self.ki * self.integral + self.kd * derivative,
            -MAX_CORRECTION_US,
            MAX_CORRECTION_US,
        )
    }
}

struct KalmanAxis {
    angle: f32,
    bias: f32,
    p00: f32,
    p01: f32,
    p10: f32,
    p11: f32,
}

impl KalmanAxis {
    const fn new() -> Self {
        Self {
            angle: 0.0,
            bias: 0.0,
            p00: 0.0,
            p01: 0.0,
            p10: 0.0,
            p11: 0.0,
        }
    }

    fn update(&mut self, gyro_rate_dps: f32, accel_angle_deg: f32, dt: f32) -> f32 {
        const Q_ANGLE: f32 = 0.01;
        const Q_BIAS: f32 = 0.003;
        const R_MEASURE: f32 = 0.03;

        let rate = gyro_rate_dps - self.bias;
        self.angle += dt * rate;

        self.p00 += dt * (dt * self.p11 - self.p01 - self.p10 + Q_ANGLE);
        self.p01 -= dt * self.p11;
        self.p10 -= dt * self.p11;
        self.p11 += Q_BIAS * dt;

        let s = self.p00 + R_MEASURE;
        let k0 = self.p00 / s;
        let k1 = self.p10 / s;
        let y = accel_angle_deg - self.angle;

        self.angle += k0 * y;
        self.bias += k1 * y;

        let p00 = self.p00;
        let p01 = self.p01;
        self.p00 -= k0 * p00;
        self.p01 -= k0 * p01;
        self.p10 -= k1 * p00;
        self.p11 -= k1 * p01;

        self.angle
    }
}

struct FlightController {
    roll_pid: Pid,
    pitch_pid: Pid,
    roll_filter: KalmanAxis,
    pitch_filter: KalmanAxis,
}

impl FlightController {
    const fn new() -> Self {
        Self {
            roll_pid: Pid::new(18.0, 0.12, 1.8),
            pitch_pid: Pid::new(18.0, 0.12, 1.8),
            roll_filter: KalmanAxis::new(),
            pitch_filter: KalmanAxis::new(),
        }
    }

    fn step(&mut self, receiver: Receiver, measured: Attitude, dt: f32) -> (Attitude, Motors) {
        let roll_estimate = self
            .roll_filter
            .update(measured.roll_rate_dps, measured.roll_deg, dt);
        let pitch_estimate =
            self.pitch_filter
                .update(measured.pitch_rate_dps, measured.pitch_deg, dt);

        let roll_error = receiver.roll_deg - roll_estimate;
        let pitch_error = receiver.pitch_deg - pitch_estimate;
        let roll_correction = self.roll_pid.update(roll_error, dt);
        let pitch_correction = self.pitch_pid.update(pitch_error, dt);

        let motors = mix(receiver.throttle_us, roll_correction, pitch_correction);
        (
            Attitude {
                roll_deg: roll_estimate,
                pitch_deg: pitch_estimate,
                roll_rate_dps: measured.roll_rate_dps,
                pitch_rate_dps: measured.pitch_rate_dps,
            },
            motors,
        )
    }
}

struct Plant {
    roll_deg: f32,
    pitch_deg: f32,
    roll_rate_dps: f32,
    pitch_rate_dps: f32,
}

impl Plant {
    const fn new() -> Self {
        Self {
            roll_deg: -7.0,
            pitch_deg: 5.5,
            roll_rate_dps: 0.0,
            pitch_rate_dps: 0.0,
        }
    }

    fn measure(&self, tick: u32) -> Attitude {
        let vibration = pseudo_noise(tick) * 0.35;
        Attitude {
            roll_deg: self.roll_deg + vibration,
            pitch_deg: self.pitch_deg - vibration * 0.7,
            roll_rate_dps: self.roll_rate_dps + vibration * 4.0,
            pitch_rate_dps: self.pitch_rate_dps - vibration * 3.0,
        }
    }

    fn apply(&mut self, motors: Motors, dt: f32) {
        let left = motors.front_left + motors.rear_left;
        let right = motors.front_right + motors.rear_right;
        let front = motors.front_left + motors.front_right;
        let rear = motors.rear_left + motors.rear_right;

        let roll_accel = (right - left) * 0.55 - self.roll_rate_dps * 1.6;
        let pitch_accel = (rear - front) * 0.55 - self.pitch_rate_dps * 1.6;

        self.roll_rate_dps += roll_accel * dt;
        self.pitch_rate_dps += pitch_accel * dt;
        self.roll_deg += self.roll_rate_dps * dt;
        self.pitch_deg += self.pitch_rate_dps * dt;
    }
}

fn main() {
    announce("flight-sim: KG-style Rust no_std controller skeleton");
    announce("flight-sim: synthetic FlySky+IMU in, mixed ESC microseconds out");

    run_scenario("baseline-15ms", 15.0, 180, 18);
    run_scenario("fast-1_5ms", 1.5, 1800, 180);

    announce("flight-sim: done");
}

fn run_scenario(name: &str, dt_ms: f32, ticks: u32, report_every: u32) {
    let mut controller = FlightController::new();
    let mut plant = Plant::new();
    let dt = dt_ms / 1000.0;
    let mut max_abs_roll_error = 0.0f32;
    let mut max_abs_pitch_error = 0.0f32;

    logl::log(
        level::INFO,
        format_args!(
            "flight-sim: scenario={} dt_ms={:.1} ticks={}",
            name, dt_ms, ticks
        ),
    );

    for tick in 0..ticks {
        let receiver = receiver_frame(tick, dt);
        let measured = plant.measure(tick);
        let (estimate, motors) = controller.step(receiver, measured, dt);
        plant.apply(motors, dt);

        let roll_error = abs(receiver.roll_deg - estimate.roll_deg);
        let pitch_error = abs(receiver.pitch_deg - estimate.pitch_deg);
        if roll_error > max_abs_roll_error {
            max_abs_roll_error = roll_error;
        }
        if pitch_error > max_abs_pitch_error {
            max_abs_pitch_error = pitch_error;
        }

        if tick % report_every == 0 || tick + 1 == ticks {
            logl::log(
                level::INFO,
                format_args!(
                    "flight-sim: {} tick={} cmd=({:.1},{:.1}) est=({:.2},{:.2}) err=({:.2},{:.2}) motors=[{:.0},{:.0},{:.0},{:.0}]",
                    name,
                    tick,
                    receiver.roll_deg,
                    receiver.pitch_deg,
                    estimate.roll_deg,
                    estimate.pitch_deg,
                    roll_error,
                    pitch_error,
                    motors.front_left,
                    motors.front_right,
                    motors.rear_left,
                    motors.rear_right
                ),
            );
        }
    }

    logl::log(
        level::INFO,
        format_args!(
            "flight-sim: summary={} final=({:.2},{:.2}) max_abs_err=({:.2},{:.2})",
            name, plant.roll_deg, plant.pitch_deg, max_abs_roll_error, max_abs_pitch_error
        ),
    );
}

fn receiver_frame(tick: u32, dt: f32) -> Receiver {
    let t = tick as f32 * dt;
    let (roll, pitch) = if t < 0.45 {
        (0.0, 0.0)
    } else if t < 1.35 {
        (8.0, -4.0)
    } else if t < 2.15 {
        (-5.0, 3.0)
    } else {
        (0.0, 0.0)
    };

    Receiver {
        throttle_us: BASE_THROTTLE_US,
        roll_deg: roll,
        pitch_deg: pitch,
    }
}

fn mix(throttle: f32, roll: f32, pitch: f32) -> Motors {
    Motors {
        front_left: clamp(throttle - pitch - roll, MOTOR_MIN_US, MOTOR_MAX_US),
        front_right: clamp(throttle - pitch + roll, MOTOR_MIN_US, MOTOR_MAX_US),
        rear_left: clamp(throttle + pitch - roll, MOTOR_MIN_US, MOTOR_MAX_US),
        rear_right: clamp(throttle + pitch + roll, MOTOR_MIN_US, MOTOR_MAX_US),
    }
}

fn pseudo_noise(tick: u32) -> f32 {
    let mut x = tick.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    x ^= x >> 16;
    let bucket = (x & 0xff) as i32 - 128;
    bucket as f32 / 128.0
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn abs(value: f32) -> f32 {
    if value < 0.0 { -value } else { value }
}

fn announce(message: &str) {
    logl::log(level::INFO, message);
}
