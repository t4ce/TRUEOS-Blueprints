//! Tyche: small pseudo-random generator for blueprint apps.
//!
//! `SoftRng` is for UI variation, games, randomized retries, and demos. It is
//! not a cryptographic RNG. The seed path intentionally uses the same small
//! blueprint import style as time instead of reaching into kernel RNG internals.

unsafe extern "C" {
    fn trueos_time_monotonic_nanos() -> u64;
}

const SPLITMIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
static TYCHE_SEED_SALT: u8 = 0xA7;

#[derive(Clone, Copy, Debug)]
pub struct SoftRng {
    state: u64,
}

impl SoftRng {
    pub fn new() -> Self {
        let local = 0u8;
        let stack_addr = (&local as *const u8 as usize) as u64;
        let salt_addr = (&TYCHE_SEED_SALT as *const u8 as usize) as u64;
        Self::from_seed(mix_seed(
            monotonic_nanos(),
            stack_addr.rotate_left(17) ^ salt_addr.rotate_right(7),
        ))
    }

    pub const fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { SPLITMIX_GAMMA } else { seed },
        }
    }

    pub fn reseed(&mut self, seed: u64) {
        self.state = if seed == 0 { SPLITMIX_GAMMA } else { seed };
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        mix64(self.state)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn usize_below(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            return 0;
        }

        let upper64 = upper as u64;
        let zone = u64::MAX - (u64::MAX % upper64);
        loop {
            let value = self.next_u64();
            if value < zone {
                return (value % upper64) as usize;
            }
        }
    }

    pub fn bool(&mut self) -> bool {
        (self.next_u64() & 1) != 0
    }

    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for idx in (1..values.len()).rev() {
            let swap_with = self.usize_below(idx + 1);
            values.swap(idx, swap_with);
        }
    }
}

impl Default for SoftRng {
    fn default() -> Self {
        Self::new()
    }
}

pub fn soft_rng() -> SoftRng {
    SoftRng::new()
}

#[inline]
fn monotonic_nanos() -> u64 {
    unsafe { trueos_time_monotonic_nanos() }
}

#[inline]
const fn mix_seed(a: u64, b: u64) -> u64 {
    a ^ b ^ 0xD1B5_4A32_D192_ED03
}

#[inline]
fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}
