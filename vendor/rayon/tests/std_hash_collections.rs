use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, DefaultHasher};

#[test]
fn std_hash_set_supports_owned_and_borrowed_parallel_iteration() {
    // A non-default hasher exercises the generic std collection adapter.
    let set: HashSet<_, BuildHasherDefault<DefaultHasher>> = (0..64).collect();
    assert_eq!(set.par_iter().copied().sum::<i32>(), 2016);
    assert_eq!(set.into_par_iter().sum::<i32>(), 2016);
}

#[test]
fn std_hash_map_supports_owned_borrowed_and_mutable_parallel_iteration() {
    let mut map: HashMap<_, _, BuildHasherDefault<DefaultHasher>> =
        (0..64).map(|key| (key, -key)).collect();
    assert_eq!(map.par_iter().map(|(&key, _)| key).sum::<i32>(), 2016);
    map.par_iter_mut().for_each(|(&key, value)| *value += key);
    assert_eq!(map.into_par_iter().map(|(_, value)| value).sum::<i32>(), 0);
}

#[test]
fn std_hash_collections_drain_without_losing_capacity() {
    let mut set: HashSet<_> = (0..64).collect();
    let set_capacity = set.capacity();
    assert_eq!(set.par_drain().sum::<i32>(), 2016);
    assert!(set.is_empty());
    assert_eq!(set.capacity(), set_capacity);

    let mut map: HashMap<_, _> = (0..64).map(|key| (key, key * 2)).collect();
    let map_capacity = map.capacity();
    assert_eq!(map.par_drain().map(|(_, value)| value).sum::<i32>(), 4032);
    assert!(map.is_empty());
    assert_eq!(map.capacity(), map_capacity);
}

#[test]
fn std_hash_collect_and_extend_preserve_values_and_duplicate_key_order() {
    let mut map: HashMap<_, _> = (0..100).into_par_iter().map(|i| (i % 7, i)).collect();
    assert_eq!(map[&0], 98);
    assert_eq!(map[&1], 99);
    map.par_extend([(0, 100), (0, 101)].into_par_iter());
    assert_eq!(map[&0], 101);
    let borrowed = [(0, 102), (8, 103)];
    map.par_extend(borrowed.par_iter().map(|(key, value)| (key, value)));
    assert_eq!(map[&0], 102);
    assert_eq!(map[&8], 103);

    let mut set: HashSet<_> = (0..100).into_par_iter().map(|i| i % 7).collect();
    set.par_extend([7, 7, 8].into_par_iter());
    set.par_extend(&[8, 9]);
    assert_eq!(set, (0..10).collect());
}

#[cfg(target_os = "trueos")]
#[test]
fn native_hashbrown_set_remains_supported() {
    let mut set: hashbrown::HashSet<_> = (0..10).into_par_iter().collect();
    assert_eq!(set.par_iter().copied().sum::<i32>(), 45);
    set.par_extend([10, 11].into_par_iter());
    set.par_extend(&[11, 12]);
    assert_eq!(set.clone().into_par_iter().sum::<i32>(), 78);
    let capacity = set.capacity();
    assert_eq!(set.par_drain().sum::<i32>(), 78);
    assert!(set.is_empty());
    assert_eq!(set.capacity(), capacity);
}

#[cfg(target_os = "trueos")]
#[test]
fn native_hashbrown_map_remains_supported() {
    let mut map: hashbrown::HashMap<_, _> = (0..10).into_par_iter().map(|i| (i, -i)).collect();
    assert_eq!(map.par_iter().map(|(&key, _)| key).sum::<i32>(), 45);
    map.par_iter_mut().for_each(|(&key, value)| *value += key);
    map.par_extend([(10, 10)].into_par_iter());
    let borrowed = [(11, 11)];
    map.par_extend(borrowed.par_iter().map(|(key, value)| (key, value)));
    assert_eq!(
        map.clone()
            .into_par_iter()
            .map(|(_, value)| value)
            .sum::<i32>(),
        21
    );
    let capacity = map.capacity();
    assert_eq!(map.par_drain().map(|(_, value)| value).sum::<i32>(), 21);
    assert!(map.is_empty());
    assert_eq!(map.capacity(), capacity);
}
