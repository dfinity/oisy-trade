use std::collections::{BTreeMap, VecDeque};

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

mod iter {
    use super::map_of;
    use crate::order::queue::OrderQueueIter;
    use std::collections::{BTreeMap, VecDeque};

    #[test]
    fn yields_nothing_for_empty_map() {
        let map: BTreeMap<u32, VecDeque<&str>> = BTreeMap::new();
        assert_eq!(OrderQueueIter::new(&map).count(), 0);
    }

    #[test]
    fn yields_single_entry_for_one_level_with_one_value() {
        let map = map_of([(1u32, vec!["a"])]);
        let items: Vec<_> = OrderQueueIter::new(&map).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a")]);
    }

    #[test]
    fn preserves_fifo_within_a_level() {
        let map = map_of([(1u32, vec!["a", "b", "c"])]);
        let items: Vec<_> = OrderQueueIter::new(&map).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a"), (1, "b"), (1, "c")]);
    }

    #[test]
    fn walks_levels_in_ascending_key_order() {
        let map = map_of([(3u32, vec!["c"]), (1, vec!["a"]), (2, vec!["b"])]);
        let items: Vec<_> = OrderQueueIter::new(&map).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a"), (2, "b"), (3, "c")]);
    }

    #[test]
    fn interleaves_levels_and_preserves_fifo() {
        let map = map_of([(1u32, vec!["a", "b"]), (2, vec!["c", "d"])]);
        let items: Vec<_> = OrderQueueIter::new(&map).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a"), (1, "b"), (2, "c"), (2, "d")]);
    }

    #[test]
    fn skips_empty_inner_deques() {
        let map = map_of([(1u32, vec![]), (2, vec!["b"]), (3, vec![])]);
        let items: Vec<_> = OrderQueueIter::new(&map).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(2, "b")]);
    }

    #[test]
    fn returns_none_repeatedly_after_exhaustion() {
        let map = map_of([(1u32, vec!["a"])]);
        let mut iter = OrderQueueIter::new(&map);
        assert!(iter.next().is_some());
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }
}

mod new {
    use crate::order::queue::OrderQueue;

    #[test]
    fn is_empty_when_new() {
        let queue: OrderQueue<u32, &str> = OrderQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }
}

mod push_back {
    use crate::order::queue::OrderQueue;

    #[test]
    fn groups_into_levels() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        queue.push_back(1, "b");
        queue.push_back(2, "c");
        assert_eq!(queue.len(), 2);
        let items: Vec<_> = queue.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a"), (1, "b"), (2, "c")]);
    }
}

mod pop_front {
    use crate::order::queue::OrderQueue;

    #[test]
    fn drains_in_fifo_order() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        queue.push_back(1, "b");
        queue.push_back(2, "c");
        assert_eq!(queue.pop_front(), Some((1, "a")));
        assert_eq!(queue.pop_front(), Some((1, "b")));
        assert_eq!(queue.pop_front(), Some((2, "c")));
        assert_eq!(queue.pop_front(), None);
    }

    #[test]
    fn removes_emptied_level() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        queue.push_back(2, "b");
        assert_eq!(queue.pop_front(), Some((1, "a")));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().next(), Some((&2, &"b")));
    }
}

mod front_mut {
    use crate::order::queue::OrderQueue;

    #[test]
    fn returns_best_without_popping() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        queue.push_back(1, "b");
        let (k, v) = queue.front_mut().unwrap();
        assert_eq!(k, 1);
        assert_eq!(*v, "a");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().count(), 2);
    }
}

mod remove {
    use crate::order::queue::OrderQueue;

    #[test]
    fn matches_first_value_in_level() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        queue.push_back(1, "b");
        let removed = queue.remove(1, |v| *v == "b");
        assert_eq!(removed, Some("b"));
        let items: Vec<_> = queue.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(1, "a")]);
    }

    #[test]
    fn drops_level_when_last_value_removed() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        let removed = queue.remove(1, |v| *v == "a");
        assert_eq!(removed, Some("a"));
        assert!(queue.is_empty());
    }

    #[test]
    fn returns_none_for_absent_level() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        assert_eq!(queue.remove(2, |_| true), None);
    }

    #[test]
    fn returns_none_when_predicate_matches_nothing() {
        let mut queue: OrderQueue<u32, &str> = OrderQueue::new();
        queue.push_back(1, "a");
        assert_eq!(queue.remove(1, |v| *v == "z"), None);
    }
}

mod from_levels {
    use super::map_of;
    use crate::order::queue::OrderQueue;

    #[test]
    fn drops_empty_levels() {
        let queue = OrderQueue::from_levels(map_of([(1u32, vec![]), (2, vec!["b"]), (3, vec![])]));
        // Empty levels are dropped, so the best level is non-empty and pop_front
        // does not trap.
        assert_eq!(queue.len(), 1);
        let items: Vec<_> = queue.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(items, vec![(2, "b")]);
    }
}
