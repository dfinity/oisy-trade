use super::{GlobalOrderSeq, OrderId, OrderStatus, Price, Quantity, Side};
use crate::Timestamp;
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

/// Record of every order from submission through terminal state, plus a
/// secondary per-user index over it.
///
/// Two stable maps kept together here rather than split across the caller:
/// - `orders`: the primary store, keyed by [`OrderId`].
/// - `by_user`: a per-user index keyed by `(owner, u64::MAX - global_seq)`,
///   so a forward range scan over an owner's prefix yields that user's orders
///   newest-first. The value is the [`OrderId`], pointing back into `orders`.
///
/// The invariant is asymmetric: `by_user` mirrors **insertion only** — every
/// [`Self::insert_once`] writes both maps, but `set_status` and cancel/fill
/// update only `orders` and never touch `by_user` (its value is the immutable
/// [`OrderId`], so status transitions don't affect it).
pub struct OrderHistory<M: Memory> {
    orders: StableBTreeMap<OrderId, OrderRecord, M>,
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

    /// Insert a new order record and index it under its owner with the given
    /// canister-global insertion `seq` (which orders the per-user index
    /// newest-first). Panics if the order ID is already present.
    pub fn insert_once(&mut self, id: OrderId, seq: GlobalOrderSeq, record: OrderRecord) {
        bench_scopes!("order_history", "order_history::insert_once");
        let owner = record.owner;
        assert_eq!(
            self.orders.insert(id, record),
            None,
            "BUG: duplicate order ID {id}"
        );
        assert_eq!(
            self.by_user.insert(UserOrderKey::from_seq(owner, seq), id),
            None,
            "BUG: duplicate user-order index entry for {owner} seq {}",
            seq.get()
        );
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<OrderRecord> {
        bench_scopes!("order_history", "order_history::get");
        self.orders.get(id)
    }

    /// Updates the status of an existing order. Panics if the order is unknown.
    pub fn set_status(&mut self, id: &OrderId, status: OrderStatus) {
        bench_scopes!("order_history", "order_history::set_status");
        let mut record = self
            .orders
            .get(id)
            .unwrap_or_else(|| panic!("BUG: order {id} missing from order_history"));
        record.status = status;
        self.orders.insert(*id, record);
    }

    /// Returns up to `length` of `owner`'s orders, newest first, after skipping
    /// the first `start`.
    pub fn orders_by_user(&self, owner: Principal, start: usize, length: usize) -> Vec<OrderId> {
        bench_scopes!("order_history", "order_history::orders_by_user");
        self.by_user
            .range(UserOrderKey::newest(owner)..=UserOrderKey::oldest(owner))
            .skip(start)
            .take(length)
            .map(|entry| entry.value())
            .collect()
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (OrderId, OrderRecord)> + '_ {
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

/// Key into the per-user index: `owner` followed by the complement of the
/// canister-global insertion sequence. Encoded so byte order matches
/// `(owner, newest-first)`, which is what [`OrderHistory::orders_by_user`]
/// relies on for its forward range scan.
///
/// `Ord` is implemented by hand to match the [`Storable`] byte order
/// (`StableBTreeMap` requires `K: Ord`). The derived field-wise ordering would
/// *disagree* with the bytes — the length-prefix byte sorts before the
/// principal bytes — so we mirror the encoding: length, then principal bytes,
/// then `rev_seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UserOrderKey {
    owner: Principal,
    rev_seq: u64,
}

impl Ord for UserOrderKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let (a, b) = (self.owner.as_slice(), other.owner.as_slice());
        a.len()
            .cmp(&b.len())
            .then_with(|| a.cmp(b))
            .then_with(|| self.rev_seq.cmp(&other.rev_seq))
    }
}

impl PartialOrd for UserOrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl UserOrderKey {
    fn from_seq(owner: Principal, seq: GlobalOrderSeq) -> Self {
        Self {
            owner,
            rev_seq: u64::MAX - seq.get(),
        }
    }

    /// Lower bound of `owner`'s range — the newest possible order.
    fn newest(owner: Principal) -> Self {
        Self { owner, rev_seq: 0 }
    }

    /// Upper bound of `owner`'s range — the oldest possible order.
    fn oldest(owner: Principal) -> Self {
        Self {
            owner,
            rev_seq: u64::MAX,
        }
    }
}

/// Principals are at most 29 bytes.
const PRINCIPAL_MAX_LEN: usize = 29;
/// 1 length byte + the (zero-padded) principal + 8 bytes of `rev_seq`.
const USER_ORDER_KEY_LEN: usize = 1 + PRINCIPAL_MAX_LEN + 8;

impl Storable for UserOrderKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let principal = self.owner.as_slice();
        let mut buf = [0u8; USER_ORDER_KEY_LEN];
        buf[0] = principal.len() as u8;
        buf[1..1 + principal.len()].copy_from_slice(principal);
        buf[1 + PRINCIPAL_MAX_LEN..].copy_from_slice(&self.rev_seq.to_be_bytes());
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
        let len = bytes[0] as usize;
        assert!(
            len <= PRINCIPAL_MAX_LEN,
            "UserOrderKey principal length {len} exceeds {PRINCIPAL_MAX_LEN}"
        );
        let owner = Principal::from_slice(&bytes[1..1 + len]);
        let rev_seq = u64::from_be_bytes(
            bytes[1 + PRINCIPAL_MAX_LEN..]
                .try_into()
                .expect("8-byte slice"),
        );
        Self { owner, rev_seq }
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
        for (id, record) in self.iter() {
            assert_eq!(fresh.orders.insert(id, record), None);
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
mod tests {
    use super::*;

    /// `UserOrderKey`'s manual `Ord` must agree with its `Storable` byte order,
    /// since `StableBTreeMap` relies on that consistency.
    #[test]
    fn user_order_key_ord_matches_storable_bytes() {
        let keys = [
            // `[2]` vs `[1, 0]`: derived field-wise `Ord` would disagree with
            // the length-prefixed bytes here — the case this impl guards.
            UserOrderKey::from_seq(Principal::from_slice(&[2]), GlobalOrderSeq::new(0)),
            UserOrderKey::from_seq(Principal::from_slice(&[1, 0]), GlobalOrderSeq::new(0)),
            UserOrderKey::from_seq(Principal::from_slice(&[1]), GlobalOrderSeq::new(5)),
            UserOrderKey::from_seq(Principal::from_slice(&[1]), GlobalOrderSeq::new(9)),
            UserOrderKey::newest(Principal::anonymous()),
            UserOrderKey::oldest(Principal::anonymous()),
        ];
        for a in &keys {
            for b in &keys {
                assert_eq!(
                    a.cmp(b),
                    a.to_bytes().cmp(&b.to_bytes()),
                    "Ord disagrees with Storable bytes for {a:?} vs {b:?}"
                );
            }
        }
    }
}
