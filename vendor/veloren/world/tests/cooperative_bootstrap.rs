//! Standalone tests of the production bootstrap helpers (no asset/DB setup).
//! rustc --test --edition=2024 --cfg 'feature="cooperative-worldgen"' -O \
//!   world/tests/cooperative_bootstrap.rs -o /tmp/veloren-bootstrap-tests
#[path = "../src/generation.rs"]
mod generation;
#[path = "../src/civ/peak.rs"]
mod peak;

fn all_pairs(peaks: &[([i32; 2], u32)], limit: i32) -> Vec<bool> {
    peaks.iter().enumerate().map(|(i, &(position, height))| {
        peaks.iter().enumerate().any(|(k, &(other, other_height))| {
            let dx = i128::from(position[0]) - i128::from(other[0]);
            let dy = i128::from(position[1]) - i128::from(other[1]);
            i != k && height <= other_height && dx * dx + dy * dy < i128::from(limit)
        })
    }).collect()
}

#[test]
fn spatial_peak_rule_matches_all_pairs_across_seeds_and_thresholds() {
    for seed in 1..=12u64 {
        let mut state = seed;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u32
        };
        let points = (0..600).map(|_| {
            ([next() as i32 % 1000, next() as i32 % 1000], next() % 80)
        }).collect::<Vec<_>>();
        for limit in [0, 1, 25, 300, 10_000] {
            assert_eq!(peak::removals(&points, limit, || {}), all_pairs(&points, limit));
        }
    }
}

#[test]
fn equal_heights_strict_boundary_and_removed_neighbours_are_preserved() {
    let equal = [([0, 0], 90), ([3, 4], 90)];
    assert_eq!(peak::removals(&equal, 25, || {}), [false, false]);
    assert_eq!(peak::removals(&equal, 26, || {}), [true, true]);
    let chain = [([0, 0], 90), ([10, 0], 100), ([20, 0], 110)];
    assert_eq!(peak::removals(&chain, 300, || {}), [true, true, false]);
}

#[test]
fn negative_bucket_edges_and_extreme_coordinates_remain_correct() {
    let points = [([-18, -1], 10), ([-17, 0], 11), ([0, -1], 11),
                  ([i32::MIN, 0], 10), ([i32::MAX, 0], 10)];
    for limit in [1, 25, 300, i32::MAX] {
        assert_eq!(peak::removals(&points, limit, || {}), all_pairs(&points, limit));
    }
}

#[test]
fn sparse_large_peak_set_does_not_do_all_pairs_work() {
    let points = (0..10_000).map(|i| ([i * 64, i * 64], 10)).collect::<Vec<_>>();
    let mut units = 0;
    let removals = peak::removals(&points, 300, || units += 1);
    assert!(removals.iter().all(|removed| !removed));
    assert!(units < points.len() * 12, "performed {units} work units for {} peaks", points.len());
}

#[test]
fn site_phase_finishes_each_borrowed_update_before_trade() {
    let mut sites = [10, 20, 30, 40];
    let mut visited = Vec::new();
    generation::for_each_site(sites.iter_mut().enumerate(), |(id, value)| {
        *value += id;
        visited.push(id);
    });
    assert_eq!(visited, [0, 1, 2, 3]);
    assert_eq!(sites, [10, 21, 32, 43]);
    let delivered = sites.iter().sum::<usize>();
    assert_eq!(delivered, 106);
}

#[test]
fn map_sampling_preserves_positions_absent_cells_and_stateful_order() {
    let mut seed = 7u64;
    let mut sample = |position| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (position % 3 != 0).then_some((position, seed))
    };
    let actual = generation::collect_ordered(0..4096, &mut sample);
    let final_seed = seed;
    let mut expected_seed = 7u64;
    let expected = (0..4096).map(|position| {
        expected_seed = expected_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (position % 3 != 0).then_some((position, expected_seed))
    }).collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(final_seed, expected_seed);
    assert_eq!(actual.len(), 4096);
}

#[test]
fn lod_cells_preserve_complete_non_square_grid_and_generated_values() {
    for size in [[3, 5], [32, 32], [0, 5], [5, 0]] {
        let expected = (0..size[0]).flat_map(|x| (0..size[1]).map(move |y| [x, y])).collect::<Vec<_>>();
        let generated = std::cell::RefCell::new(Vec::new());
        let mut inserted = Vec::new();
        generation::for_each_cell(size, |position| {
            generated.borrow_mut().push(position);
            position[0] * 37 + position[1]
        }, |position, value| {
            assert_eq!(generated.borrow().last(), Some(&position));
            inserted.push((position, value));
        });
        assert_eq!(*generated.borrow(), expected);
        assert_eq!(inserted, expected.iter().map(|p| (*p, p[0] * 37 + p[1])).collect::<Vec<_>>());
    }
}
