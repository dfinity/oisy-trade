use super::{OrderId, OrderStatus, Price, Quantity, Side};
use crate::Timestamp;
use crate::user::UserId;
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;

/// Record of an order from submission through terminal state.
///
/// Persisted in a [`ic_stable_structures::StableBTreeMap`] keyed by [`OrderId`].
/// Once the canister is launched the CBOR layout becomes an upgrade-durable
/// schema: removing or renumbering a field — or adding one without an
/// `Option<T>` / `#[cbor(default)]` fallback — breaks decoding of records
/// written by prior canister versions. Before launch there are no persisted
/// records to preserve, so schema-breaking changes are acceptable.
/// The trading pair is deliberately not stored — it is derivable from the
/// `OrderBookId` embedded in the [`OrderId`] via the trading-pair registry.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct OrderRecord {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub owner: Principal,
    #[n(1)]
    pub side: Side,
    #[n(2)]
    pub price: Price,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub status: OrderStatus,
    /// Submission time, taken from the add-limit-order event. Display-only —
    /// no matching or ordering logic reads it.
    #[n(5)]
    pub timestamp: Timestamp,
}

impl From<OrderRecord> for dex_types::OrderRecord {
    fn from(record: OrderRecord) -> Self {
        dex_types::OrderRecord {
            owner: record.owner,
            side: record.side.into(),
            price: record.price.into(),
            quantity: record.quantity.into(),
            status: record.status.into(),
            timestamp: record.timestamp.as_nanos(),
        }
    }
}

impl Storable for OrderRecord {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("order record encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("order record encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode order record bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// Stored value of [`OrderHistory`]'s primary map: an [`OrderRecord`] paired
/// with the canister-global insertion sequence assigned when it was first
/// inserted. The sequence orders the per-user index newest-first and lets
/// `get_my_orders` resolve an `OrderId` cursor back to its index position. It's
/// an index bookkeeping concern, so it lives in this wrapper rather than as a
/// field on the domain `OrderRecord`.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
struct SeqEntry {
    #[n(0)]
    seq: u64,
    #[n(1)]
    record: OrderRecord,
}

impl Storable for SeqEntry {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("seq entry encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("seq entry encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode seq entry bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// Record of every order from submission through terminal state, plus a
/// secondary per-user index over it.
///
/// Two stable maps kept together here rather than split across the caller:
/// - `orders`: the primary store, keyed by [`OrderId`].
/// - `by_user`: a per-user index keyed by `(user, u64::MAX - global_seq)`,
///   so a forward range scan over a user's prefix yields that user's orders
///   newest-first. The value is the [`OrderId`], pointing back into `orders`.
///
/// The invariant is asymmetric: `by_user` mirrors **insertion only** — every
/// [`Self::insert_once`] writes both maps, but `set_status` and cancel/fill
/// update only `orders` and never touch `by_user` (its value is the immutable
/// [`OrderId`], so status transitions don't affect it).
pub struct OrderHistory<M: Memory> {
    orders: StableBTreeMap<OrderId, SeqEntry, M>,
    by_user: StableBTreeMap<UserOrderKey, OrderId, M>,
}

impl<M: Memory> std::fmt::Debug for OrderHistory<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderHistory")
            .field("len", &self.orders.len())
            .finish()
    }
}

impl<M: Memory> OrderHistory<M> {
    /// `orders_memory` and `by_user_memory` **must be distinct memory regions**:
    /// the two maps share no isolation beyond their backing memory, so passing
    /// the same handle twice would let them overwrite each other.
    pub fn new(orders_memory: M, by_user_memory: M) -> Self {
        Self {
            orders: StableBTreeMap::init(orders_memory),
            by_user: StableBTreeMap::init(by_user_memory),
        }
    }

    /// Insert a new order record and index it under `user`. The order's
    /// canister-global insertion sequence — which orders the per-user index
    /// newest-first — is the current order count: the index is insert-only, so
    /// the count is a dense, monotonic sequence.
    /// Panics if the order ID is present.
    pub fn insert_once(&mut self, id: OrderId, user: UserId, record: OrderRecord) {
        bench_scopes!("order_history", "order_history::insert_once");
        let seq = self.orders.len();
        assert_eq!(
            self.orders.insert(id, SeqEntry { seq, record }),
            None,
            "BUG: duplicate order ID {id}"
        );
        assert_eq!(
            self.by_user.insert(UserOrderKey::from_seq(user, seq), id),
            None,
            "BUG: duplicate user-order index entry for {user:?} seq {seq}"
        );
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<OrderRecord> {
        bench_scopes!("order_history", "order_history::get");
        self.orders.get(id).map(|entry| entry.record)
    }

    /// Updates the status of an existing order. Panics if the order is unknown.
    pub fn set_status(&mut self, id: &OrderId, status: OrderStatus) {
        bench_scopes!("order_history", "order_history::set_status");
        let mut entry = self
            .orders
            .get(id)
            .unwrap_or_else(|| panic!("BUG: order {id} missing from order_history"));
        entry.record.status = status;
        self.orders.insert(*id, entry);
    }

    /// Returns up to `length` of `user`'s orders, newest first, resuming
    /// strictly after the `after` order (a cursor from a prior page) — or from
    /// the newest when `after` is `None`. An `after` naming an unknown order
    /// yields an empty page. Each page is an `O(length)` range scan from the
    /// cursor (no offset to re-walk), so retrieving a whole history is linear
    /// in its size.
    pub fn orders_after(
        &self,
        user: UserId,
        after: Option<OrderId>,
        length: usize,
    ) -> Vec<OrderId> {
        bench_scopes!("order_history", "order_history::orders_after");
        use std::ops::Bound;
        let lower = match after {
            None => Bound::Included(UserOrderKey::newest(user)),
            Some(cursor) => match self.orders.get(&cursor) {
                Some(entry) => Bound::Excluded(UserOrderKey::from_seq(user, entry.seq)),
                None => return Vec::new(),
            },
        };
        self.by_user
            .range((lower, Bound::Included(UserOrderKey::oldest(user))))
            .take(length)
            .map(|entry| entry.value())
            .collect()
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (OrderId, SeqEntry)> + '_ {
        self.orders
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }

    #[cfg(test)]
    fn user_index_iter(&self) -> impl Iterator<Item = (UserOrderKey, OrderId)> + '_ {
        self.by_user
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

/// Key into the per-user index: the interned [`UserId`] followed by the
/// complement of the canister-global insertion sequence, so a forward range
/// scan over a user's prefix yields their orders newest-first.
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// already matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UserOrderKey {
    user: UserId,
    rev_seq: u64,
}

impl UserOrderKey {
    fn from_seq(user: UserId, seq: u64) -> Self {
        Self {
            user,
            rev_seq: u64::MAX - seq,
        }
    }

    /// Lower bound of `user`'s range — the newest possible order.
    fn newest(user: UserId) -> Self {
        Self { user, rev_seq: 0 }
    }

    /// Upper bound of `user`'s range — the oldest possible order.
    fn oldest(user: UserId) -> Self {
        Self {
            user,
            rev_seq: u64::MAX,
        }
    }
}

/// 8 bytes of `UserId` + 8 bytes of `rev_seq`, both big-endian.
const USER_ORDER_KEY_LEN: usize = 8 + 8;

impl Storable for UserOrderKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; USER_ORDER_KEY_LEN];
        buf[..8].copy_from_slice(&self.user.get().to_be_bytes());
        buf[8..].copy_from_slice(&self.rev_seq.to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            USER_ORDER_KEY_LEN,
            "UserOrderKey must decode from exactly {USER_ORDER_KEY_LEN} bytes"
        );
        let user = UserId::new(u64::from_be_bytes(
            bytes[..8].try_into().expect("8-byte slice"),
        ));
        let rev_seq = u64::from_be_bytes(bytes[8..].try_into().expect("8-byte slice"));
        Self { user, rev_seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: USER_ORDER_KEY_LEN as u32,
        is_fixed_size: true,
    };
}

#[cfg(test)]
impl Clone for OrderHistory<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (id, entry) in self.iter() {
            assert_eq!(fresh.orders.insert(id, entry), None);
        }
        for (key, id) in self.user_index_iter() {
            assert_eq!(fresh.by_user.insert(key, id), None);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for OrderHistory<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter()) && self.user_index_iter().eq(other.user_index_iter())
    }
}

#[cfg(test)]
impl Eq for OrderHistory<ic_stable_structures::VectorMemory> {}

#[cfg(test)]
mod tests;
