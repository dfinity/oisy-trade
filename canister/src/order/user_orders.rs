use super::OrderId;
use candid::Principal;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;

#[cfg(test)]
mod tests;

/// Per-user secondary index over the orders a principal has placed, ordered
/// newest-first.
///
/// The key is `(owner, u64::MAX - global_seq)`: prefixing by `owner` keeps each
/// user's entries contiguous, and storing the *complement* of the global
/// insertion sequence makes a plain forward range scan return the most recent
/// order first — no reliance on reverse iteration. The value is the
/// [`OrderId`], which points back to the full record in
/// [`super::OrderHistory`].
pub struct UserOrders<M: Memory> {
    index: StableBTreeMap<UserOrderKey, OrderId, M>,
}

impl<M: Memory> std::fmt::Debug for UserOrders<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserOrders")
            .field("len", &self.index.len())
            .finish()
    }
}

impl<M: Memory> UserOrders<M> {
    pub fn new(memory: M) -> Self {
        Self {
            index: StableBTreeMap::init(memory),
        }
    }

    /// Records that `owner` placed `order_id` with global insertion sequence
    /// `seq`. Panics if the key already exists.
    pub fn insert(&mut self, owner: Principal, seq: u64, order_id: OrderId) {
        assert_eq!(
            self.index
                .insert(UserOrderKey::from_seq(owner, seq), order_id),
            None,
            "BUG: duplicate user-order index entry for {owner} seq {seq}"
        );
    }

    /// Returns up to `length` of `owner`'s orders, newest first, after skipping
    /// the first `start`.
    pub fn page(&self, owner: Principal, start: usize, length: usize) -> Vec<OrderId> {
        self.index
            .range(UserOrderKey::newest(owner)..=UserOrderKey::oldest(owner))
            .skip(start)
            .take(length)
            .map(|entry| entry.value())
            .collect()
    }
}

/// Key into [`UserOrders`]: `owner` followed by the complement of the global
/// insertion sequence. Encoded so that byte order matches
/// `(owner, newest-first)`, which is what the range scan in
/// [`UserOrders::page`] relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct UserOrderKey {
    owner: Principal,
    rev_seq: u64,
}

impl UserOrderKey {
    fn from_seq(owner: Principal, seq: u64) -> Self {
        Self {
            owner,
            rev_seq: u64::MAX - seq,
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
const KEY_LEN: usize = 1 + PRINCIPAL_MAX_LEN + 8;

impl Storable for UserOrderKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let principal = self.owner.as_slice();
        let mut buf = [0u8; KEY_LEN];
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
            KEY_LEN,
            "UserOrderKey must decode from exactly {KEY_LEN} bytes"
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
        max_size: KEY_LEN as u32,
        is_fixed_size: true,
    };
}

#[cfg(test)]
impl UserOrders<ic_stable_structures::VectorMemory> {
    fn iter(&self) -> impl Iterator<Item = (UserOrderKey, OrderId)> + '_ {
        self.index.iter().map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for UserOrders<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(ic_stable_structures::VectorMemory::default());
        for (key, order_id) in self.iter() {
            assert_eq!(fresh.index.insert(key, order_id), None);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for UserOrders<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl Eq for UserOrders<ic_stable_structures::VectorMemory> {}
