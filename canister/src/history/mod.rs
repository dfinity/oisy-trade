use crate::ids::{CompositeId, Seq, SeqMarker};
use crate::user::UserId;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;
use std::fmt;
use std::ops::Bound as RangeBound;

#[cfg(test)]
mod tests;

/// Marker for the per-store insertion sequence: within a single [`History`]
/// instance (one memory region), each inserted record gets the next dense,
/// monotonic value. It is not shared across stores — every [`History`] starts
/// its sequence at zero — so the value is only per-store monotonic, not
/// canister-wide.
pub struct InsertionSeqMarker;
impl SeqMarker for InsertionSeqMarker {
    const NAME: &'static str = "InsertionSeq";
}

/// Per-store insertion sequence assigned to each inserted record: monotonic
/// within a single [`History`] instance, not across stores.
type InsertionSeq = Seq<InsertionSeqMarker>;

/// Key into the per-user index: the interned [`UserId`] followed by the
/// per-store insertion sequence, so a range scan over a user's prefix
/// yields their records oldest-first — [`History::page_by_user`] reverses it for
/// newest-first. The value is the primary key, pointing back into the primary
/// map. Both components are fixed-width big-endian via [`CompositeId`], so the
/// derived field-wise `Ord` matches the byte order `StableBTreeMap` relies on.
type ByUserKey = CompositeId<UserId, InsertionSeq>;

impl ByUserKey {
    /// Lower bound of `user`'s range — the oldest possible record.
    fn first_of(user: UserId) -> Self {
        Self::new(user, InsertionSeq::ZERO)
    }

    /// Upper bound of `user`'s range — the newest possible record.
    fn last_of(user: UserId) -> Self {
        Self::new(user, InsertionSeq::new(u64::MAX))
    }
}

/// A record persistable in a [`History`] store: anything with a context-free
/// minicbor codec.
pub trait Record: minicbor::Encode<()> + for<'a> minicbor::Decode<'a, ()> {}
impl<T: minicbor::Encode<()> + for<'a> minicbor::Decode<'a, ()>> Record for T {}

/// Stored value of a [`History`]'s primary map: a record paired with the
/// per-store insertion sequence assigned when it was first inserted. The
/// sequence keys the per-user index (scanned in reverse for newest-first) and
/// lets [`History::page_by_user`] resolve a primary-key cursor back to its index
/// position. It's an index-bookkeeping concern, so it lives in this wrapper
/// rather than as a field on the domain record.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
struct SeqRecord<V> {
    #[n(0)]
    seq: InsertionSeq,
    #[n(1)]
    record: V,
}

impl<V: Record> Storable for SeqRecord<V> {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("history record encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("history record encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode history record bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// The `after` cursor passed to a paginated reader names a record that is
/// unknown or does not belong to the querying user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorNotFound;

/// Append-and-index record store shared by [`crate::order::OrderHistory`] and
/// [`crate::order::TradeHistory`]:
/// - `primary`: the canonical store, keyed by `K`, values seq-stamped.
/// - `by_user`: a per-user index keyed by `(UserId, per-store seq)`, so a
///   reverse prefix scan yields a user's records newest-first.
///
/// The two maps are kept in lockstep by [`History::insert_once`], which is the
/// only way to add a record; the insertion sequence is the current record
/// count, so it is a dense, per-store monotonic sequence.
pub struct History<M, K, V>
where
    M: Memory,
    K: Storable + Ord + Clone,
    V: Record,
{
    primary: StableBTreeMap<K, SeqRecord<V>, M>,
    by_user: StableBTreeMap<ByUserKey, K, M>,
}

impl<M, K, V> History<M, K, V>
where
    M: Memory,
    K: Storable + Ord + Clone + fmt::Debug,
    V: Record,
{
    /// `primary_memory` and `by_user_memory` **must be distinct memory
    /// regions**: the two maps share no isolation beyond their backing memory.
    pub fn new(primary_memory: M, by_user_memory: M) -> Self {
        Self {
            primary: StableBTreeMap::init(primary_memory),
            by_user: StableBTreeMap::init(by_user_memory),
        }
    }

    /// Number of records in the primary map.
    pub fn len(&self) -> u64 {
        self.primary.len()
    }

    /// Returns a copy of the record stored under `key`, or `None` if absent.
    pub fn get(&self, key: &K) -> Option<V> {
        self.primary.get(key).map(|entry| entry.record)
    }

    /// Returns whether a record is stored under `key`.
    pub fn contains_key(&self, key: &K) -> bool {
        self.primary.contains_key(key)
    }

    /// Insert a new record under `user`, indexing it in the per-user map. The
    /// record's insertion sequence — which orders the per-user index, reverse
    /// scanned for newest-first — is the current record count. Panics if `key`
    /// is already present.
    pub fn insert_once(&mut self, user: UserId, key: K, record: V) {
        let seq = InsertionSeq::new(self.primary.len());
        assert!(
            self.primary
                .insert(key.clone(), SeqRecord { seq, record })
                .is_none(),
            "BUG: duplicate history key {key:?}"
        );
        assert!(
            self.by_user
                .insert(ByUserKey::new(user, seq), key)
                .is_none(),
            "BUG: duplicate user index entry for {user:?} seq {seq:?}"
        );
    }

    /// Read-modify-write the record under `key`. `f` mutates the record in place
    /// and returns whether it changed; only a changed record is written back.
    /// Panics if `key` is absent.
    pub fn modify(&mut self, key: &K, f: impl FnOnce(&mut V) -> bool) {
        let mut entry = self
            .primary
            .get(key)
            .unwrap_or_else(|| panic!("BUG: history key {key:?} missing"));
        if f(&mut entry.record) {
            self.primary.insert(key.clone(), entry);
        }
    }

    /// Returns up to `length` of `user`'s primary keys in newest-first order.
    /// With `after: None` the page starts at the newest record; otherwise
    /// `after` is a cursor — the last key of the previous page — and the page
    /// continues with the next-older record. An `after` that names an unknown
    /// record — or one that does not belong to `user` — yields [`CursorNotFound`];
    /// a valid cursor with no older records is `Ok(vec![])`. Each page is an
    /// `O(length)` range scan.
    pub fn page_by_user(
        &self,
        user: UserId,
        after: Option<K>,
        length: usize,
    ) -> Result<Vec<K>, CursorNotFound> {
        let upper = match after {
            None => RangeBound::Included(ByUserKey::last_of(user)),
            Some(cursor) => {
                let entry = self.primary.get(&cursor).ok_or(CursorNotFound)?;
                let key = ByUserKey::new(user, entry.seq);
                // The cursor must be one of `user`'s own records: its key must
                // map back to it in the index. A cursor from another user (or a
                // forged key) resolves to a seq whose `(user, seq)` key isn't in
                // the index — reject it rather than scan from a bogus position.
                if self.by_user.get(&key) != Some(cursor) {
                    return Err(CursorNotFound);
                }
                RangeBound::Excluded(key)
            }
        };
        Ok(self
            .by_user
            .range((RangeBound::Included(ByUserKey::first_of(user)), upper))
            .rev()
            .take(length)
            .map(|entry| entry.value())
            .collect())
    }

    /// Returns up to `length` primary entries with keys in `[lower, upper]`, in
    /// newest-first (descending key) order. Lets a caller whose primary key
    /// embeds a scannable prefix list a sub-range directly off the primary map.
    pub fn range_primary(&self, lower: K, upper: RangeBound<K>, length: usize) -> Vec<(K, V)> {
        self.primary
            .range((RangeBound::Included(lower), upper))
            .rev()
            .take(length)
            .map(|entry| (entry.key().clone(), entry.value().record))
            .collect()
    }

    /// Iterates the primary store's `(key, record)` entries in key order.
    pub fn iter_primary(&self) -> impl Iterator<Item = (K, V)> + '_ {
        self.primary
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().record))
    }

    /// Iterates the per-user index as `(user, insertion sequence, key)` in index
    /// order.
    pub fn iter_by_user(&self) -> impl Iterator<Item = (UserId, u64, K)> + '_ {
        self.by_user.iter().map(|entry| {
            let key = entry.key();
            (*key.first(), key.second().get(), entry.value())
        })
    }
}

#[cfg(test)]
impl<K, V> History<ic_stable_structures::VectorMemory, K, V>
where
    K: Storable + Ord + Clone + fmt::Debug,
    V: Record + Clone,
{
    fn iter(&self) -> impl Iterator<Item = (K, SeqRecord<V>)> + '_ {
        self.primary
            .iter()
            .map(|entry| (entry.key().clone(), entry.value()))
    }

    fn user_index_iter(&self) -> impl Iterator<Item = (ByUserKey, K)> + '_ {
        self.by_user
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl<K, V> Clone for History<ic_stable_structures::VectorMemory, K, V>
where
    K: Storable + Ord + Clone + fmt::Debug,
    V: Record + Clone,
{
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (key, entry) in self.iter() {
            assert!(fresh.primary.insert(key, entry).is_none());
        }
        for (key, id) in self.user_index_iter() {
            assert!(fresh.by_user.insert(key, id).is_none());
        }
        fresh
    }
}

#[cfg(test)]
impl<K, V> PartialEq for History<ic_stable_structures::VectorMemory, K, V>
where
    K: Storable + Ord + Clone + fmt::Debug + PartialEq,
    V: Record + Clone + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter()) && self.user_index_iter().eq(other.user_index_iter())
    }
}

#[cfg(test)]
impl<K, V> Eq for History<ic_stable_structures::VectorMemory, K, V>
where
    K: Storable + Ord + Clone + fmt::Debug + Eq,
    V: Record + Clone + Eq,
{
}
