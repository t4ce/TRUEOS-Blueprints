use glam::Vec3;

use crate::animation::SAMPLE_HZ;

pub(super) const WIND_FRAMES: usize = 15 * SAMPLE_HZ as usize;
pub(super) const IDLE_FRAMES: usize = WIND_FRAMES + 5 * SAMPLE_HZ as usize;

// World-space acceleration in metres per second squared. The avatar faces -Z.
const TRAILING_BREEZE: Vec3 = Vec3::new(2.1, 0.0, -1.0);
const GUSTS: [(f32, Vec3); 9] = [
    (0.0, Vec3::ZERO),
    (3.0, Vec3::new(3.4, 0.0, -1.6)),
    (5.0, Vec3::new(0.8, 0.0, -0.35)),
    (7.0, Vec3::new(-1.3, 0.0, 0.8)),
    (9.0, Vec3::new(0.9, 0.0, 0.25)),
    (11.0, Vec3::new(4.0, 0.0, -1.9)),
    (13.0, TRAILING_BREEZE),
    // Leave some momentum for the hair to settle visibly after the wind stops.
    (14.55, TRAILING_BREEZE),
    (15.0, Vec3::ZERO),
];

pub(super) fn acceleration(frame: usize) -> Vec3 {
    if frame >= WIND_FRAMES {
        return Vec3::ZERO;
    }
    let seconds = frame as f32 / SAMPLE_HZ as f32;
    let next = GUSTS.partition_point(|&(time, _)| time <= seconds);
    let (start, from) = GUSTS[next - 1];
    let (end, to) = GUSTS[next];
    let t = (seconds - start) / (end - start);
    let smooth = t * t * (3.0 - 2.0 * t);
    from.lerp(to, smooth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_starts_calm_and_leaves_five_seconds_to_settle() {
        assert_eq!(IDLE_FRAMES - WIND_FRAMES, 5 * SAMPLE_HZ as usize);
        assert_eq!(acceleration(0), Vec3::ZERO);
        assert!(acceleration(WIND_FRAMES - 1).length() < 0.02);
        for frame in WIND_FRAMES..=IDLE_FRAMES {
            assert_eq!(acceleration(frame), Vec3::ZERO);
        }
    }

    #[test]
    fn main_gust_is_side_dominant_and_the_wind_turns_gently() {
        let at_second = |seconds| acceleration(seconds * SAMPLE_HZ as usize);
        let main = at_second(11);
        assert!(main.z < 0.0 && main.x > -main.z);
        let reverse = at_second(7);
        assert!(reverse.x < 0.0 && reverse.z > 0.0);
        assert!(reverse.length() < main.length() * 0.4);
        assert!(at_second(5).length() < main.length() * 0.25);
        for frame in 1..=IDLE_FRAMES {
            assert!(acceleration(frame).distance(acceleration(frame - 1)) < 0.18);
        }
    }
}
