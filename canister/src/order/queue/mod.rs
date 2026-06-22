use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque, btree_map, vec_deque};

#[cfg(test)]
mod tests;

/// Per-side order book storage: a [`BTreeMap`] of price levels, each holding a
/// FIFO [`VecDeque`] of orders.
///
/// Mutating operations keep levels non-empty: any operation that empties a
/// level removes the level from the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderQueue<K, V> {
    levels: BTreeMap<K, VecDeque<V>>,
}

impl<K, V> OrderQueue<K, V> {
    pub fn new() -> Self {
        Self {
            levels: BTreeMap::new(),
        }
    }

    /// Builds an `OrderQueue` from existing levels, **dropping any empty
    /// level** so the non-empty-levels invariant holds regardless of the input
    /// (e.g. a snapshot or an externally-constructed map). Without this, an
    /// empty `VecDeque` at the best price would later trap in
    /// `pop_front`/`front_mut`.
    pub fn from_levels(mut levels: BTreeMap<K, VecDeque<V>>) -> Self
    where
        K: Ord,
    {
        levels.retain(|_, queue| !queue.is_empty());
        Self { levels }
    }

    pub fn levels(&self) -> impl Iterator<Item = (&K, &VecDeque<V>)> + '_ {
        self.levels.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn iter(&self) -> OrderQueueIter<'_, K, V> {
        OrderQueueIter::new(&self.levels)
    }
}

impl<K: Ord + Copy, V> OrderQueue<K, V> {
    /// Pop the front of the best (first) level. Returns `None` if empty.
    /// Removes the level if its queue becomes empty.
    pub fn pop_front(&mut self) -> Option<(K, V)> {
        let mut entry = self.levels.first_entry()?;
        let key = *entry.key();
        let value = entry
            .get_mut()
            .pop_front()
            .expect("BUG: empty queue at price level");
        if entry.get().is_empty() {
            entry.remove();
        }
        Some((key, value))
    }

    /// Mutable reference to the front of the best level. Does not mutate the
    /// structure of the queue; the level is left in place.
    pub fn front_mut(&mut self) -> Option<(K, &mut V)> {
        let (&key, queue) = self.levels.iter_mut().next()?;
        let value = queue.front_mut().expect("BUG: empty queue at price level");
        Some((key, value))
    }

    pub fn push_back(&mut self, key: K, value: V) {
        self.levels.entry(key).or_default().push_back(value);
    }

    /// Find and remove the first value in level `key` matching `predicate`.
    /// Removes the level if its queue becomes empty. Returns `None` if the
    /// level is absent or no value matches.
    pub fn remove<F: FnMut(&V) -> bool>(&mut self, key: K, mut predicate: F) -> Option<V> {
        let Entry::Occupied(mut entry) = self.levels.entry(key) else {
            return None;
        };
        let queue = entry.get_mut();
        let pos = queue.iter().position(&mut predicate)?;
        let removed = queue.remove(pos).expect("position is valid");
        if queue.is_empty() {
            entry.remove();
        }
        Some(removed)
    }
}

impl<K, V> Default for OrderQueue<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct OrderQueueIter<'a, K, V> {
    outer: btree_map::Iter<'a, K, VecDeque<V>>,
    inner: Option<(&'a K, vec_deque::Iter<'a, V>)>,
}

impl<'a, K, V> OrderQueueIter<'a, K, V> {
    pub fn new(map: &'a BTreeMap<K, VecDeque<V>>) -> Self {
        Self {
            outer: map.iter(),
            inner: None,
        }
    }
}

impl<'a, K, V> Iterator for OrderQueueIter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((k, it)) = self.inner.as_mut()
                && let Some(v) = it.next()
            {
                return Some((k, v));
            }
            let (k, q) = self.outer.next()?;
            self.inner = Some((k, q.iter()));
        }
    }
}
