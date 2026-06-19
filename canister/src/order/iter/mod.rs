use std::collections::{BTreeMap, VecDeque, btree_map, vec_deque};

#[cfg(test)]
mod tests;

pub struct OrderIter<'a, K, V> {
    outer: btree_map::Iter<'a, K, VecDeque<V>>,
    inner: Option<(&'a K, vec_deque::Iter<'a, V>)>,
}

impl<'a, K, V> OrderIter<'a, K, V> {
    pub fn new(map: &'a BTreeMap<K, VecDeque<V>>) -> Self {
        Self {
            outer: map.iter(),
            inner: None,
        }
    }
}

impl<'a, K, V> Iterator for OrderIter<'a, K, V> {
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
