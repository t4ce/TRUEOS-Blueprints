//! Flock population and the timing of independent tier changes.

pub const TIER_COUNT: usize = 8;
pub const CAPACITY: usize = 10 * TIER_COUNT;
pub const FRAME_PERIOD_NS: u64 = 60_000_000_000;

const TIER_STEP: u8 = 16;
const TINY: u8 = (TIER_COUNT as u8 - 1) * TIER_STEP;
const OUTSIDE: u8 = 127;
const SEED_MASK: u8 = 127;
const MAX_PROGRESS: u32 = 255;
const MOVE_BASE_SECONDS: f32 = 4.4;
const MOVE_BASE_NS: u64 = 4_400_000_000;
// Integer-percent speed factors wrap across whole 12-second flight laps.
const FLIGHT_CLOCK_PERIOD_NS: u64 = 1_200_000_000_000;
const MIN_DWELL_NS: u64 = 3_000_000_000;
const GROWTH_INTERVAL_NS: u64 = 1_250_000_000;
// Keep the closer tiers from accumulating all the butterflies. The tiny tier
// can accept every new arrival until the total population reaches capacity.
const TIER_LIMIT: [usize; TIER_COUNT - 1] = [4, 5, 6, 7, 8, 9, 10];

fn tier(coordinate: u8) -> usize {
    // Group fractional positions by the nearest reference tier, with
    // half-way positions in the closer band.
    usize::from((coordinate + TIER_STEP / 2 - 1) / TIER_STEP)
}

#[derive(Clone, Copy, Default)]
struct Butterfly {
    seed: u8,
    source: u8,
    target: u8,
    changed_at: u64,
}

impl Butterfly {
    fn moving(self) -> bool {
        self.source != self.target
    }

    fn move_duration(self) -> u64 {
        // Match the hull shader's integer-percent orbit and movement rates.
        let speed_percent = 85 + (u64::from(self.seed) * 17 + 13) % 31;
        MOVE_BASE_NS * 100 / speed_percent
    }

    fn start_move(&mut self, target: u8, now: u64) {
        self.source = self.target;
        self.target = target;
        self.changed_at = now;
    }

    fn ready_to_grow(self, now: u64, pressure: usize) -> bool {
        let stagger_ms = (u64::from(self.seed) * 2593 + u64::from(self.target) * 1597) % 4000;
        let dwell = MIN_DWELL_NS + stagger_ms * 1_000_000 / (pressure - 1).min(4) as u64;
        now - self.changed_at >= dwell
    }

    fn growth_target(self, occupancy: &Occupancy) -> Option<u8> {
        let roll = u16::from(self.seed) * 11 + u16::from(self.target) * 3;
        let step = 8 + (roll % 9) as u8;
        let normal = self.target.saturating_sub(step);
        let target = if roll % 13 == 0 {
            normal.saturating_sub(TIER_STEP)
        } else {
            normal
        };
        let source_tier = tier(self.target);
        [target, normal].into_iter().find(|&destination| {
            let target_tier = tier(destination);
            target_tier == source_tier || occupancy.reserved[target_tier] < TIER_LIMIT[target_tier]
        })
    }

    fn record(self, now: u64) -> f32 {
        let progress = if self.moving() {
            let duration = self.move_duration();
            ((now - self.changed_at).min(duration) * u64::from(MAX_PROGRESS) / duration) as u32
        } else {
            MAX_PROGRESS
        };
        // Carry bitwise records as finite f32 values in the camera block.
        // Coordinate 127 marks the source of an arrival or the target of a departure.
        f32::from_bits(
            0x2000_0000
                | (u32::from(self.seed) << 22)
                | (u32::from(self.source) << 15)
                | (u32::from(self.target) << 8)
                | progress,
        )
    }
}

#[derive(Default)]
struct Occupancy {
    reserved: [usize; TIER_COUNT],
    settled: [usize; TIER_COUNT],
    departing: bool,
}

impl Occupancy {
    fn count(butterflies: &[Butterfly]) -> Self {
        let mut occupancy = Self::default();
        for &butterfly in butterflies {
            if butterfly.target == OUTSIDE {
                occupancy.departing = true;
                continue;
            }
            let tier = tier(butterfly.target);
            occupancy.reserved[tier] += 1;
            if !butterfly.moving() {
                occupancy.settled[tier] += 1;
            }
        }
        occupancy
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub spawned: usize,
    pub promoted: usize,
    pub departed: usize,
}

pub struct Snapshot {
    pub records: [f32; CAPACITY],
    pub motion: [f32; 4],
}

pub struct Flock {
    butterflies: [Butterfly; CAPACITY],
    count: usize,
    active_seeds: u128,
    next_seed: u8,
    pending: u32,
    started: u64,
    frame_time: u64,
    last_growth: [Option<u64>; TIER_COUNT],
    departure_count: u32,
}

impl Flock {
    pub fn new(now: u64) -> Self {
        let mut butterflies = [Butterfly::default(); CAPACITY];
        for (tier, butterfly) in butterflies[..TIER_COUNT].iter_mut().enumerate() {
            *butterfly = Butterfly {
                seed: tier as u8,
                source: tier as u8 * TIER_STEP,
                target: tier as u8 * TIER_STEP,
                changed_at: now,
            };
        }
        Self {
            butterflies,
            count: TIER_COUNT,
            active_seeds: (1 << TIER_COUNT) - 1,
            next_seed: TIER_COUNT as u8,
            pending: 0,
            started: now,
            frame_time: now,
            last_growth: [None; TIER_COUNT],
            departure_count: 0,
        }
    }

    pub fn click(&mut self) {
        self.pending = self.pending.saturating_add(1);
    }

    pub fn population(&self) -> usize {
        self.count
    }

    pub fn tick(&mut self, now: u64) -> Events {
        let now = now.max(self.frame_time);
        self.frame_time = now;
        self.finish_moves(now);

        let spawned = (CAPACITY - self.count).min(self.pending as usize);
        for _ in 0..spawned {
            self.spawn(now);
        }
        self.pending -= spawned as u32;

        let mut occupancy = Occupancy::count(&self.butterflies[..self.count]);
        let promoted = self.grow(now, &mut occupancy);
        let departed = usize::from(self.try_departure(now, &occupancy));
        Events {
            spawned,
            promoted,
            departed,
        }
    }

    fn finish_moves(&mut self, now: u64) {
        let mut index = 0;
        while index < self.count {
            let butterfly = &mut self.butterflies[index];
            if butterfly.moving() {
                let duration = butterfly.move_duration();
                if now - butterfly.changed_at >= duration {
                    if butterfly.target == OUTSIDE {
                        self.active_seeds &= !(1 << butterfly.seed);
                        self.count -= 1;
                        self.butterflies.swap(index, self.count);
                        continue;
                    }
                    butterfly.source = butterfly.target;
                    butterfly.changed_at += duration;
                }
            }
            index += 1;
        }
    }

    fn spawn(&mut self, now: u64) {
        let mut seed = self.next_seed;
        while self.active_seeds & (1 << seed) != 0 {
            seed = (seed + 1) & SEED_MASK;
        }
        self.active_seeds |= 1 << seed;
        self.next_seed = (seed + 1) & SEED_MASK;
        self.butterflies[self.count] = Butterfly {
            seed,
            source: OUTSIDE,
            target: TINY,
            changed_at: now,
        };
        self.count += 1;
    }

    fn grow(&mut self, now: u64, occupancy: &mut Occupancy) -> usize {
        let mut pressure = 0;
        let mut promoted = 0;
        for source_tier in (1..TIER_COUNT).rev() {
            // One occupant is the baseline for each tier. Extra settled
            // butterflies here and in smaller tiers increase growth pressure.
            pressure += occupancy.settled[source_tier].saturating_sub(1);
            if pressure < 2
                || occupancy.settled[source_tier] < 2
                || self.last_growth[source_tier].is_some_and(|last| now - last < GROWTH_INTERVAL_NS)
            {
                continue;
            }
            let Some((index, _, target)) = self.butterflies[..self.count]
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, butterfly)| {
                    !butterfly.moving()
                        && tier(butterfly.target) == source_tier
                        && butterfly.ready_to_grow(now, pressure)
                })
                .filter_map(|(index, butterfly)| {
                    butterfly
                        .growth_target(occupancy)
                        .map(|target| (index, butterfly.changed_at, target))
                })
                .min_by_key(|&(_, changed_at, _)| changed_at)
            else {
                continue;
            };

            self.butterflies[index].start_move(target, now);
            self.last_growth[source_tier] = Some(now);
            occupancy.settled[source_tier] -= 1;
            occupancy.reserved[source_tier] -= 1;
            occupancy.reserved[tier(target)] += 1;
            promoted += 1;
        }
        promoted
    }

    fn try_departure(&mut self, now: u64, occupancy: &Occupancy) -> bool {
        if self.count < CAPACITY || self.pending == 0 || occupancy.departing {
            return false;
        }

        // Prefer an older grown butterfly. Occasionally choose an undergrown
        // one, and keep a settled butterfly at each occupied tier.
        let early = self.departure_count % 5 == 4;
        let Some(butterfly) = self.butterflies[..self.count]
            .iter_mut()
            .filter(|butterfly| {
                !butterfly.moving() && occupancy.settled[tier(butterfly.target)] > 1
            })
            .min_by_key(|butterfly| {
                (
                    early && tier(butterfly.target) < 2,
                    butterfly.target,
                    butterfly.changed_at,
                )
            })
        else {
            return false;
        };
        butterfly.start_move(OUTSIDE, now);
        self.departure_count = self.departure_count.wrapping_add(1);
        true
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut records = [0.0; CAPACITY];
        for (record, butterfly) in records.iter_mut().zip(&self.butterflies[..self.count]) {
            *record = butterfly.record(self.frame_time);
        }
        let elapsed = (self.frame_time - self.started) % FLIGHT_CLOCK_PERIOD_NS;
        Snapshot {
            records,
            motion: [
                elapsed as f32 * 1e-9,
                MOVE_BASE_SECONDS,
                MOVE_BASE_SECONDS,
                self.count as f32,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000_000;

    fn decode(word: f32) -> (u32, u32, u32, u32) {
        let bits = word.to_bits();
        (
            bits >> 22 & 127,
            bits >> 15 & 127,
            bits >> 8 & 127,
            bits & MAX_PROGRESS,
        )
    }

    #[test]
    fn a_click_adds_one_arrival_and_leaves_the_existing_butterflies_where_they_were() {
        let mut flock = Flock::new(1);
        let initial = flock.snapshot();
        assert_eq!(initial.motion, [0.0, 4.4, 4.4, 8.0]);
        assert_eq!(flock.tick(1 + 10 * SECOND), Events::default());

        flock.click();
        assert_eq!(flock.tick(1 + 11 * SECOND).spawned, 1);
        let arrival = flock.snapshot();
        assert_eq!(initial.records[..8], arrival.records[..8]);
        assert_eq!(decode(arrival.records[8]), (8, 127, 112, 0));
        assert_eq!(arrival.motion[3], 9.0);

        assert_eq!(flock.tick(1 + 20 * SECOND), Events::default());
        assert_eq!(decode(flock.snapshot().records[8]), (8, 112, 112, 255));
        assert_eq!(flock.population(), 9);
    }

    #[test]
    fn settled_crowding_propagates_gradually_without_further_clicks() {
        let mut flock = Flock::new(0);
        flock.click();
        flock.click();
        assert_eq!(flock.tick(0).spawned, 2);
        assert_eq!(flock.tick(2 * SECOND), Events::default());
        assert_eq!(flock.tick(6 * SECOND).promoted, 1);
        assert_eq!(decode(flock.snapshot().records[7]), (7, 112, 96, 0));
        assert_eq!(flock.tick(9 * SECOND), Events::default());
        assert_eq!(flock.tick(20 * SECOND).promoted, 1);
        assert_eq!(decode(flock.snapshot().records[6]), (6, 96, 85, 0));
        assert_eq!(flock.population(), 10);
    }

    #[test]
    fn every_seed_moves_at_least_five_times_slower_and_records_round_trip() {
        let mut shortest = u64::MAX;
        let mut longest = 0;
        for seed in 0..=SEED_MASK {
            let butterfly = Butterfly {
                seed,
                source: OUTSIDE,
                target: TINY,
                changed_at: 0,
            };
            let duration = butterfly.move_duration();
            shortest = shortest.min(duration);
            longest = longest.max(duration);
            let record = butterfly.record(duration / 2);
            assert!(record.is_finite());
            assert_eq!(decode(record), (u32::from(seed), 127, 112, 127));
        }
        assert_eq!(shortest, MOVE_BASE_NS * 100 / 115);
        assert_eq!(longest, MOVE_BASE_NS * 100 / 85);
        assert!(shortest >= 5 * 750_000_000);
    }

    #[test]
    fn fractional_growth_can_land_between_tiers_even_with_a_full_band() {
        let butterfly = Butterfly {
            seed: 8,
            source: 103,
            target: 103,
            changed_at: 0,
        };
        let mut occupancy = Occupancy::default();
        occupancy.reserved[6] = TIER_LIMIT[6];
        assert_eq!(tier(butterfly.target), 6);
        assert_eq!(butterfly.growth_target(&occupancy), Some(94));
        assert_eq!(tier(94), 6);
    }

    #[test]
    fn all_eighty_butterflies_draw_and_extra_clicks_wait_for_a_departure() {
        let mut flock = Flock::new(0);
        for _ in 0..CAPACITY - TIER_COUNT {
            flock.click();
        }
        assert_eq!(flock.tick(0).spawned, CAPACITY - TIER_COUNT);
        assert_eq!(flock.tick(6 * SECOND).departed, 0);
        assert_eq!(flock.population(), CAPACITY);
        assert_eq!(flock.snapshot().motion[3], CAPACITY as f32);

        flock.click();
        let events = flock.tick(7 * SECOND);
        assert_eq!(events.departed, 1);
        assert_eq!(events.spawned, 0);
        assert_eq!(flock.population(), CAPACITY);
        assert_eq!(flock.tick(10 * SECOND).spawned, 0);
        assert_eq!(flock.tick(13 * SECOND).spawned, 1);
        assert_eq!(flock.population(), CAPACITY);
        assert_eq!(flock.pending, 0);

        let snapshot = flock.snapshot();
        let mut seen = 0u128;
        for word in snapshot.records {
            let bit = 1u128 << decode(word).0;
            assert_eq!(seen & bit, 0);
            seen |= bit;
        }
    }

    #[test]
    fn cycling_the_seed_allocator_does_not_reuse_a_live_butterfly() {
        let mut flock = Flock::new(0);
        for _ in TIER_COUNT..CAPACITY {
            flock.click();
        }
        flock.tick(0);
        for click in 1..=140 {
            flock.click();
            let now = click * 20 * SECOND;
            assert_eq!(flock.tick(now).departed, 1);
            assert_eq!(flock.tick(now + 6 * SECOND).spawned, 1);
            assert_eq!(flock.population(), CAPACITY);
            assert_eq!(flock.active_seeds.count_ones(), CAPACITY as u32);
        }
    }

    #[test]
    fn the_orbit_clock_wraps_at_a_whole_cycle_for_every_speed() {
        let mut flock = Flock::new(120 * SECOND);
        flock.tick(120 * SECOND + FLIGHT_CLOCK_PERIOD_NS);
        assert_eq!(flock.snapshot().motion[0], 0.0);
    }
}
