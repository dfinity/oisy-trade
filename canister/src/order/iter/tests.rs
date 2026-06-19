use super::OrderIter;
use std::collections::{BTreeMap, VecDeque};

#[test]
fn should_yield_nothing_for_empty_map() {
    let map: BTreeMap<u32, VecDeque<&str>> = BTreeMap::new();
    assert_eq!(OrderIter::new(&map).count(), 0);
}

#[test]
fn should_yield_single_entry_for_one_level_with_one_value() {
    let map = map_of([(1u32, vec!["a"])]);
    let items: Vec<_> = OrderIter::new(&map).map(|(k, v)| (*k, *v)).collect();
    assert_eq!(items, vec![(1, "a")]);
}

#[test]
fn should_preserve_fifo_within_a_level() {
    let map = map_of([(1u32, vec!["a", "b", "c"])]);
    let items: Vec<_> = OrderIter::new(&map).map(|(k, v)| (*k, *v)).collect();
    assert_eq!(items, vec![(1, "a"), (1, "b"), (1, "c")]);
}

#[test]
fn should_walk_levels_in_ascending_key_order() {
    let map = map_of([(3u32, vec!["c"]), (1, vec!["a"]), (2, vec!["b"])]);
    let items: Vec<_> = OrderIter::new(&map).map(|(k, v)| (*k, *v)).collect();
    assert_eq!(items, vec![(1, "a"), (2, "b"), (3, "c")]);
}

#[test]
fn should_interleave_levels_and_preserve_fifo() {
    let map = map_of([(1u32, vec!["a", "b"]), (2, vec!["c", "d"])]);
    let items: Vec<_> = OrderIter::new(&map).map(|(k, v)| (*k, *v)).collect();
    assert_eq!(items, vec![(1, "a"), (1, "b"), (2, "c"), (2, "d")]);
}

#[test]
fn should_skip_empty_inner_deques() {
    let map = map_of([(1u32, vec![]), (2, vec!["b"]), (3, vec![])]);
    let items: Vec<_> = OrderIter::new(&map).map(|(k, v)| (*k, *v)).collect();
    assert_eq!(items, vec![(2, "b")]);
}

#[test]
fn should_return_none_repeatedly_after_exhaustion() {
    let map = map_of([(1u32, vec!["a"])]);
    let mut iter = OrderIter::new(&map);
    assert!(iter.next().is_some());
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}

fn map_of<K, V, I>(entries: I) -> BTreeMap<K, VecDeque<V>>
where
    K: Ord,
    I: IntoIterator<Item = (K, Vec<V>)>,
{
    entries
        .into_iter()
        .map(|(k, vs)| (k, VecDeque::from(vs)))
        .collect()
}
