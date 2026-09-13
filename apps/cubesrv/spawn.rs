//! Server-owned temporary terrain cube followed by one VFX playback.
pub const INTERVAL_MS: u64 = 3_000;
pub const DELAY_MS: u64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub event: u32,
    pub anchor: [i16; 3],
    pub terrain: bool,
    pub frame: Option<u8>,
}

/// Sample absolute server time so late wakes never accumulate timer drift or
/// advance a previous animation into the next spawn's half-second delay.
pub fn spawn_with_vfx(elapsed_ms: u64, frames: u8, period_ms: u16) -> Spawn {
    assert!(frames > 0 && period_ms > 0);
    // Keep all authored frames. Longer strips speed up enough to leave a
    // 100 ms cleanup gap before the next three-second spawn.
    let period_ms = period_ms.min(((INTERVAL_MS - DELAY_MS - 100) / frames as u64) as u16);
    let cycle = elapsed_ms / INTERVAL_MS;
    let age = elapsed_ms % INTERVAL_MS;
    let distance = (5 + cycle % 6) as i16 * 8;
    // This sky world's terrain is distant from the center. Spawn a floating c4
    // block at the landmark's top elevation, within the requested center radius.
    let anchor = match cycle % 4 {
        0 => [distance, 8, 0], 1 => [0, 8, distance],
        2 => [-distance, 8, 0], _ => [0, 8, -distance],
    };
    let frame = age.checked_sub(DELAY_MS).map(|age| age / period_ms as u64)
        .filter(|frame| *frame < frames as u64).map(|frame| frame as u8);
    Spawn { event: cycle as u32, anchor,
        terrain: cycle > 0 && age < DELAY_MS + frames as u64 * period_ms as u64,
        frame: if cycle > 0 { frame } else { None } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cube_delay_full_loop_and_cleanup_obey_absolute_deadlines() {
        let at = |t| spawn_with_vfx(t,16,150);
        assert!(!at(2999).terrain);
        assert!(at(3000).terrain);
        assert_eq!(at(3499).frame,None);
        for frame in 0..16 { assert_eq!(at(3500+frame*150).frame,Some(frame as u8)); }
        assert_eq!(at(5899).frame,Some(15));
        assert!(!at(5900).terrain);
        assert!(at(6000).terrain);
        assert_eq!(at(6000).frame,None);
        assert_eq!(at(6400).frame,None);
    }
    #[test]
    fn positions_stay_five_to_ten_blocks_from_center() {
        for cycle in 1..=48 {
            let p = spawn_with_vfx(cycle*INTERVAL_MS,16,150).anchor;
            let d = p[0] as i32*p[0] as i32 + p[2] as i32*p[2] as i32;
            assert!((40*40..=80*80).contains(&d));
            assert_eq!(p[1],8);
        }
    }
    #[test]
    fn long_strip_plays_every_frame_before_next_spawn() {
        for frame in 0..24 {
            assert_eq!(spawn_with_vfx(3500+frame*100,24,150).frame,Some(frame as u8));
        }
        assert!(!spawn_with_vfx(5900,24,150).terrain);
        assert_eq!(spawn_with_vfx(6000,24,150).frame,None);
    }
}
