//! Fixed 32x32 sprites, encoded as pixel lifetimes [first, end) in frame units.
pub const WIDTH: u8 = 32;
pub const HEIGHT: u8 = 32;
pub const INSTANCES: usize = 6;
/// Existing cube presets: c1, c2, r1, c3, r2, c4, r3.
pub const PIXEL_SIDES_C1: [u8; 7] = [1, 2, 3, 4, 6, 8, 12];
pub const DEMO_PIXEL_SIDES_C1: [u8; 4] = [1, 2, 3, 4];
pub const CELLS: usize = 1024;
pub const PERIOD_MS: u16 = 400;
pub const DELAY_MS: u16 = 500;
pub const REST_MS: u32 = 1000;
pub const MAX_BYTES: usize = 128 * 1024;
pub const HEADER: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pixel {
    pub x: u8,
    pub y: u8,
    pub palette: u8,
}

/// A stable pixel identity across all frames in its compressed lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lifetime {
    pub pixel: Pixel,
    pub first: u8,
    pub end: u8,
}

#[derive(Clone, Copy)]
pub struct Sequence<'a> {
    bytes: &'a [u8],
    data: usize,
}
impl<'a> Sequence<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < HEADER
            || bytes.len() > MAX_BYTES
            || &bytes[..4] != b"VFX1"
            || bytes[4..7] != [1, WIDTH, HEIGHT]
            || bytes[7] == 0
            || bytes[10] == 0
            || bytes[11] != 0
        {
            return None;
        }
        let data = HEADER + bytes[10] as usize * 3;
        if data > bytes.len() || (bytes.len() - data) % 5 != 0 {
            return None;
        }
        let sequence = Self { bytes, data };
        if sequence.period_ms() == 0 || sequence.period_ms() > PERIOD_MS {
            return None;
        }
        let mut ends = [0; CELLS];
        for r in bytes[data..].chunks_exact(5) {
            if r[0] >= WIDTH
                || r[1] >= HEIGHT
                || r[2] >= bytes[10]
                || r[3] >= r[4]
                || r[4] > bytes[7]
            {
                return None;
            }
            let cell = r[1] as usize * 32 + r[0] as usize;
            if r[3] < ends[cell] {
                return None;
            }
            ends[cell] = r[4];
        }
        Some(sequence)
    }
    pub fn frame_count(self) -> u8 {
        self.bytes[7]
    }
    pub fn period_ms(self) -> u16 {
        u16::from_le_bytes([self.bytes[8], self.bytes[9]])
    }
    pub fn lifetimes(self) -> impl Iterator<Item = Lifetime> + 'a {
        self.bytes[self.data..].chunks_exact(5).map(|r| Lifetime {
            pixel: Pixel {
                x: r[0],
                y: r[1],
                palette: r[2],
            },
            first: r[3],
            end: r[4],
        })
    }
    pub fn pixels(self, frame: u8) -> impl Iterator<Item = Pixel> + 'a {
        self.bytes[self.data..]
            .chunks_exact(5)
            .filter(move |r| r[3] <= frame && frame < r[4])
            .map(|r| Pixel {
                x: r[0],
                y: r[1],
                palette: r[2],
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slot {
    pub revision: u32,
    pub bytes: u32,
    pub anchor: [i16; 3],
    pub frames: u8,
    pub period_ms: u16,
    pub pixel_side_c1: u8,
}
impl Slot {
    pub fn frame(self, age: u64) -> Option<u8> {
        age.checked_sub(DELAY_MS as u64)
            .map(|t| t / self.period_ms as u64)
            .filter(|f| *f < self.frames as u64)
            .map(|f| f as u8)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scene {
    pub gallery_revision: u32,
    pub event: u32,
    pub age_ms: u32,
    pub slots: [Slot; INSTANCES],
}
impl Scene {
    pub const BYTES: usize = 12 + 18 * INSTANCES;
    pub fn encode(self) -> [u8; Self::BYTES] {
        let mut b = [0; Self::BYTES];
        b[..4].copy_from_slice(&self.gallery_revision.to_le_bytes());
        b[4..8].copy_from_slice(&self.event.to_le_bytes());
        b[8..12].copy_from_slice(&self.age_ms.to_le_bytes());
        for (slot, r) in self.slots.iter().zip(b[12..].chunks_exact_mut(18)) {
            r[..4].copy_from_slice(&slot.revision.to_le_bytes());
            r[4..8].copy_from_slice(&slot.bytes.to_le_bytes());
            for (i, v) in slot.anchor.iter().enumerate() {
                r[8 + i * 2..10 + i * 2].copy_from_slice(&v.to_le_bytes());
            }
            r[14] = slot.frames;
            r[15..17].copy_from_slice(&slot.period_ms.to_le_bytes());
            r[17] = slot.pixel_side_c1;
        }
        b
    }
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() != Self::BYTES {
            return None;
        }
        let slots = core::array::from_fn(|i| {
            let r = &b[12 + i * 18..30 + i * 18];
            Slot {
                revision: u32::from_le_bytes(r[..4].try_into().unwrap()),
                bytes: u32::from_le_bytes(r[4..8].try_into().unwrap()),
                anchor: core::array::from_fn(|a| {
                    i16::from_le_bytes(r[8 + a * 2..10 + a * 2].try_into().unwrap())
                }),
                frames: r[14],
                period_ms: u16::from_le_bytes(r[15..17].try_into().unwrap()),
                pixel_side_c1: r[17],
            }
        });
        let scene = Self {
            gallery_revision: u32::from_le_bytes(b[..4].try_into().unwrap()),
            event: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            age_ms: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            slots,
        };
        if scene.age_ms >= scene.batch_ms()
            || slots.iter().any(|s| {
                s.bytes < HEADER as u32
                    || s.bytes > MAX_BYTES as u32
                    || !PIXEL_SIDES_C1.contains(&s.pixel_side_c1)
                    || s.frames == 0
                    || s.period_ms == 0
                    || s.period_ms > PERIOD_MS
                    || s.anchor.iter().any(|v| !(-1024..=1024).contains(v))
            })
        {
            return None;
        }
        Some(scene)
    }
    /// Announce the next effects during the one-second VFX gap, leaving the usual
    /// DELAY_MS lead-in. Thus next first frame - previous last expiry = REST_MS.
    pub fn batch_ms(self) -> u32 {
        self.slots
            .iter()
            .map(|s| s.frames as u32 * s.period_ms as u32)
            .max()
            .unwrap_or(0)
            + REST_MS
    }
    pub fn newer_than(self, old: Self) -> bool {
        let d = self.event.wrapping_sub(old.event);
        (d != 0 && d < 1 << 31) || (self.event == old.event && self.age_ms > old.age_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demo_only_rolls_the_smallest_four_presets() {
        assert_eq!(DEMO_PIXEL_SIDES_C1, PIXEL_SIDES_C1[..4]);
        assert_eq!(INSTANCES, 6);
    }
    #[test]
    fn longest_loop_controls_next_batch_with_exact_one_second_vfx_gap() {
        let slot = Slot {
            revision: 7,
            bytes: 15,
            anchor: [0, 0, 0],
            frames: 16,
            period_ms: PERIOD_MS,
            pixel_side_c1: 1,
        };
        let mut scene = Scene {
            gallery_revision: 1,
            event: 1,
            age_ms: 0,
            slots: [slot; INSTANCES],
        };
        scene.slots[5].frames = 40;
        let last_expiry = DELAY_MS as u32 + 40 * PERIOD_MS as u32;
        assert_eq!(scene.batch_ms(), 17000);
        assert_eq!(scene.batch_ms() + DELAY_MS as u32 - last_expiry, 1000);
        assert_eq!(scene.slots[5].frame(3000), Some(6)); // no forced three-second reset
        assert_eq!(scene.slots[5].frame(last_expiry as u64 - 1), Some(39));
        assert_eq!(scene.slots[5].frame(last_expiry as u64), None);
        scene.slots[5].frames = 255;
        scene.age_ms = 102500;
        assert_eq!(Scene::parse(&scene.encode()), Some(scene)); // exceeds u16 milliseconds
        assert_eq!(scene.batch_ms(), 103000);
        scene.age_ms = 103000;
        assert!(Scene::parse(&scene.encode()).is_none());
        let bytes = [
            b'V', b'F', b'X', b'1', 1, 32, 32, 255, 144, 1, 1, 0, 255, 255, 255,
        ];
        assert_eq!(Sequence::parse(&bytes).unwrap().period_ms(), 400);
    }
    #[test]
    fn size_selector_accepts_existing_tiers_without_changing_asset_identity() {
        let slot = Slot {
            revision: 7,
            bytes: 100,
            anchor: [40, 8, 0],
            frames: 2,
            period_ms: 150,
            pixel_side_c1: 1,
        };
        let mut scene = Scene {
            gallery_revision: 1,
            event: 1,
            age_ms: 0,
            slots: [slot; INSTANCES],
        };
        for size in PIXEL_SIDES_C1 {
            scene.slots[0].pixel_side_c1 = size;
            assert_eq!(Scene::parse(&scene.encode()), Some(scene));
            assert_eq!(scene.slots[0].revision, 7);
        }
        for size in [0, 5, 7, 9, 255] {
            scene.slots[0].pixel_side_c1 = size;
            assert!(Scene::parse(&scene.encode()).is_none());
        }
        assert!(Scene::parse(&[0; 112]).is_none());
    }
    #[test]
    fn six_independent_effects_expire_with_their_own_loop() {
        let slot = Slot {
            revision: 7,
            bytes: 100,
            anchor: [40, 8, 0],
            frames: 2,
            period_ms: 150,
            pixel_side_c1: 1,
        };
        let mut scene = Scene {
            gallery_revision: 1,
            event: 1,
            age_ms: 0,
            slots: [slot; INSTANCES],
        };
        scene.slots[5].frames = 10;
        let active = |age| scene.slots.map(|s| s.frame(age).is_some());
        assert_eq!(active(799), [true; INSTANCES]);
        assert_eq!(active(800), [false, false, false, false, false, true]);
        assert_eq!(active(2000), [false; INSTANCES]);
        assert_eq!(Scene::BYTES, 120);
        assert!(Scene::BYTES + 8 < 1200);
        let bytes = scene.encode();
        assert_eq!(Scene::parse(&bytes), Some(scene));
        assert!(Scene::parse(&bytes[..78]).is_none()); // former four-slot wire layout
        scene.slots[1].anchor[0] += 8;
        assert_eq!(Scene::parse(&scene.encode()), Some(scene));
    }
    #[test]
    fn lifetimes_retain_change_disappear_and_reappear() {
        let bytes = [
            b'V', b'F', b'X', b'1', 1, 32, 32, 5, 150, 0, 2, 0, 255, 0, 0, 0, 255, 0, 2, 3, 0, 0,
            3, 2, 3, 1, 3, 4, 4, 5, 0, 0, 1, 4, 5, 0, 4, 5,
        ];
        let seq = Sequence::parse(&bytes).unwrap();
        assert_eq!(
            seq.lifetimes().next().unwrap(),
            Lifetime {
                pixel: Pixel {
                    x: 2,
                    y: 3,
                    palette: 0
                },
                first: 0,
                end: 3
            }
        );
        assert_eq!(seq.pixels(0).count(), 2);
        for frame in [1, 2] {
            assert_eq!(
                seq.pixels(frame).collect::<alloc::vec::Vec<_>>(),
                [Pixel {
                    x: 2,
                    y: 3,
                    palette: 0
                }]
            );
        }
        assert_eq!(seq.pixels(3).next().unwrap().palette, 1);
        assert_eq!(
            seq.pixels(4).next().unwrap(),
            Pixel {
                x: 4,
                y: 5,
                palette: 0
            }
        );
        assert_eq!(seq.pixels(5).count(), 0);
        for (index, value) in [
            (5, 64),
            (8, 0),
            (10, 0),
            (18, 32),
            (20, 2),
            (21, 3),
            (22, 6),
            (26, 2),
        ] {
            let mut bad = bytes;
            bad[index] = value;
            assert!(Sequence::parse(&bad).is_none(), "{index}");
        }
        assert!(Sequence::parse(&bytes[..bytes.len() - 1]).is_none());
    }
    #[test]
    fn scene_roundtrip_order_and_exact_timing_boundaries() {
        let slot = Slot {
            revision: 7,
            bytes: 100,
            anchor: [40, 8, 0],
            frames: 16,
            period_ms: 150,
            pixel_side_c1: 1,
        };
        assert_eq!(slot.frame(499), None);
        assert_eq!(slot.frame(500), Some(0));
        assert_eq!(slot.frame(2899), Some(15));
        assert_eq!(slot.frame(2900), None);
        let scene = Scene {
            gallery_revision: 1,
            event: u32::MAX,
            age_ms: 0,
            slots: [slot; INSTANCES],
        };
        assert_eq!(Scene::parse(&scene.encode()), Some(scene));
        let mut next = scene;
        next.event = 0;
        assert!(next.newer_than(scene));
        assert!(!scene.newer_than(next));
        next = scene;
        next.age_ms = 50;
        assert!(next.newer_than(scene));
        assert!(!scene.newer_than(scene));
        next.age_ms = scene.batch_ms();
        assert!(Scene::parse(&next.encode()).is_none());
        next = scene;
        next.slots[1].period_ms = 0;
        assert!(Scene::parse(&next.encode()).is_none());
    }
}
