use super::{OrderId, PairToken, Price, Quantity, Side};
use crate::Timestamp;
use crate::user::UserId;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, StableCell, Storable};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// Canister-global, monotonic sequence assigned to each side-projected fill
/// record as it is appended. The two legs of one fill get two consecutive
/// values; it is never reused (fills are append-only) and orders records both
/// globally and within an order's prefix. It is the `after` cursor for the
/// per-order fill scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FillSeq(u64);

impl FillSeq {
    pub const ZERO: Self = Self(0);

    pub const fn new(seq: u64) -> Self {
        Self(seq)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("BUG: FillSeq overflow"))
    }
}

/// First-class identifier of a side-projected fill: the owning [`OrderId`]
/// followed by the canister-global [`FillSeq`]. A range scan over an `order`
/// prefix yields that order's fills in `seq` order; reversed, newest-first.
/// Mirrors [`OrderId`]: opaque outside the canister as a 48-character hex
/// string (16 bytes of `OrderId` + 8 bytes of `seq`).
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FillId {
    order: OrderId,
    seq: FillSeq,
}

/// 16 bytes of `OrderId` + 8 bytes of `seq`, both big-endian.
const FILL_ID_LEN: usize = 16 + 8;

impl FillId {
    pub fn new(order: OrderId, seq: FillSeq) -> Self {
        Self { order, seq }
    }

    pub fn seq(&self) -> FillSeq {
        self.seq
    }

    fn first(order: OrderId) -> Self {
        Self {
            order,
            seq: FillSeq::ZERO,
        }
    }

    fn last(order: OrderId) -> Self {
        Self {
            order,
            seq: FillSeq::new(u64::MAX),
        }
    }
}

impl Storable for FillId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; FILL_ID_LEN];
        buf[..16].copy_from_slice(&self.order.to_bytes());
        buf[16..].copy_from_slice(&self.seq.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            FILL_ID_LEN,
            "FillId must decode from exactly {FILL_ID_LEN} bytes"
        );
        let order = OrderId::from_bytes(Cow::Borrowed(&bytes[..16]));
        let seq = FillSeq::new(u64::from_be_bytes(
            bytes[16..].try_into().expect("8-byte slice"),
        ));
        Self { order, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: FILL_ID_LEN as u32,
        is_fixed_size: true,
    };
}

impl fmt::Display for FillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{:016x}", self.order, self.seq.get())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FillIdParseError;

impl fmt::Display for FillIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid fill ID: expected 48-character hex string")
    }
}

impl FromStr for FillId {
    type Err = FillIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 48 || !s.is_ascii() {
            return Err(FillIdParseError);
        }
        let order = OrderId::from_str(&s[..32]).map_err(|_| FillIdParseError)?;
        let seq = u64::from_str_radix(&s[32..], 16).map_err(|_| FillIdParseError)?;
        Ok(Self {
            order,
            seq: FillSeq::new(seq),
        })
    }
}

impl From<FillId> for String {
    fn from(id: FillId) -> Self {
        id.to_string()
    }
}

/// Key into the account-wide secondary index: the interned [`UserId`] followed
/// by the canister-global [`FillSeq`], so a range scan over a user's prefix
/// yields their fills oldest-first across all their orders —
/// [`FillStore::trades_after`] reverses it for newest-first. The value is the
/// [`FillId`], pointing back into the primary `fills` map.
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FillByUserKey {
    user: UserId,
    seq: FillSeq,
}

/// 8 bytes of `UserId` + 8 bytes of `seq`, both big-endian.
const FILL_BY_USER_KEY_LEN: usize = 8 + 8;

impl FillByUserKey {
    fn first(user: UserId) -> Self {
        Self {
            user,
            seq: FillSeq::ZERO,
        }
    }

    fn last(user: UserId) -> Self {
        Self {
            user,
            seq: FillSeq::new(u64::MAX),
        }
    }
}

impl Storable for FillByUserKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; FILL_BY_USER_KEY_LEN];
        buf[..8].copy_from_slice(&self.user.get().to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            FILL_BY_USER_KEY_LEN,
            "FillByUserKey must decode from exactly {FILL_BY_USER_KEY_LEN} bytes"
        );
        let user = UserId::new(u64::from_be_bytes(
            bytes[..8].try_into().expect("8-byte slice"),
        ));
        let seq = FillSeq::new(u64::from_be_bytes(
            bytes[8..].try_into().expect("8-byte slice"),
        ));
        Self { user, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: FILL_BY_USER_KEY_LEN as u32,
        is_fixed_size: true,
    };
}

/// One side-projected fill, holding everything needed to audit one of the two
/// orders' view of an execution. The counterparty is never stored.
///
/// Once the canister is launched its CBOR layout is an upgrade-durable schema;
/// pre-launch there are no persisted records, so schema changes are acceptable.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct FillRecord {
    #[n(0)]
    pub order_id: OrderId,
    #[n(1)]
    pub side: Side,
    #[n(2)]
    pub price: Price,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub notional: Quantity,
    #[n(5)]
    pub fee: Quantity,
    #[n(6)]
    pub fee_token: PairToken,
    #[n(7)]
    pub is_maker: bool,
    #[n(8)]
    pub timestamp: Timestamp,
}

impl Storable for FillRecord {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("fill record encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("fill record encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode fill record bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// The `after` cursor passed to [`FillStore::fills_after`] names a fill that is
/// unknown (no record with that global sequence in the order's prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorNotFound;

/// Append-only store of side-projected fill records, mirroring the storage
/// shape of [`crate::order::OrderHistory`]: a primary map keyed by an
/// `OrderId`-prefixed [`FillId`] (so a per-order read is a range scan), a
/// `(UserId, global_seq)` secondary index for the account-wide read, plus a
/// canister-global monotonic [`FillSeq`] counter persisted in its own cell so
/// it stays monotonic across upgrades.
pub struct FillStore<M: Memory> {
    fills: StableBTreeMap<FillId, FillRecord, M>,
    by_user: StableBTreeMap<FillByUserKey, FillId, M>,
    next_seq: StableCell<u64, M>,
}

impl<M: Memory> fmt::Debug for FillStore<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FillStore")
            .field("len", &self.fills.len())
            .field("next_seq", self.next_seq.get())
            .finish()
    }
}

impl<M: Memory> FillStore<M> {
    /// `fills_memory`, `by_user_memory`, and `seq_memory` **must be three
    /// distinct memory regions**.
    pub fn new(fills_memory: M, by_user_memory: M, seq_memory: M) -> Self {
        Self {
            fills: StableBTreeMap::init(fills_memory),
            by_user: StableBTreeMap::init(by_user_memory),
            next_seq: StableCell::init(seq_memory, 0),
        }
    }

    /// Append the two side-projected records of one fill — the taker leg owned by
    /// `taker_user` and the maker leg owned by `maker_user` — each under the next
    /// global [`FillSeq`], advancing the sequence by two. Each record is written
    /// to the primary map and indexed under its owner in `by_user` (2 + 2
    /// inserts per fill).
    pub fn append(
        &mut self,
        taker_leg: FillRecord,
        taker_user: UserId,
        maker_leg: FillRecord,
        maker_user: UserId,
    ) {
        bench_scopes!("fills", "fills::append");
        self.insert(taker_leg, taker_user);
        self.insert(maker_leg, maker_user);
    }

    fn insert(&mut self, record: FillRecord, user: UserId) {
        let seq = FillSeq::new(*self.next_seq.get());
        let id = FillId::new(record.order_id, seq);
        assert_eq!(
            self.fills.insert(id, record),
            None,
            "BUG: duplicate fill id for seq {seq:?}"
        );
        assert_eq!(
            self.by_user.insert(FillByUserKey { user, seq }, id),
            None,
            "BUG: duplicate user-fill index entry for {user:?} seq {seq:?}"
        );
        self.next_seq.set(seq.next().get());
    }

    /// Returns up to `length` of `order`'s fills, newest first. With
    /// `after: None` the page starts at the newest fill; otherwise `after` is a
    /// cursor — the last fill of the previous page — and the page continues with
    /// the next-older fill. An `after` whose sequence is not one of `order`'s
    /// fills yields [`CursorNotFound`]; a valid cursor with no older fills is
    /// `Ok(vec![])`.
    pub fn fills_after(
        &self,
        order: OrderId,
        after: Option<FillSeq>,
        length: usize,
    ) -> Result<Vec<(FillSeq, FillRecord)>, CursorNotFound> {
        bench_scopes!("fills", "fills::fills_after");
        use std::ops::Bound;
        let upper = match after {
            None => Bound::Included(FillId::last(order)),
            Some(seq) => {
                let id = FillId::new(order, seq);
                if !self.fills.contains_key(&id) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(id)
            }
        };
        Ok(self
            .fills
            .range((Bound::Included(FillId::first(order)), upper))
            .rev()
            .take(length)
            .map(|entry| (entry.key().seq, entry.value()))
            .collect())
    }

    /// Returns up to `length` of `user`'s fills across **all** their orders,
    /// newest first. With `after: None` the page starts at the newest fill;
    /// otherwise `after` is a cursor — the last fill of the previous page — and
    /// the page continues with the next-older fill. An `after` whose sequence is
    /// not one of `user`'s fills yields [`CursorNotFound`]; a valid cursor with
    /// no older fills is `Ok(vec![])`. Each page reverse-scans the `by_user`
    /// index then resolves each [`FillId`] from the primary map — the exact
    /// shape of `OrderHistory::orders_after` — so it is `O(length)`.
    pub fn trades_after(
        &self,
        user: UserId,
        after: Option<FillId>,
        length: usize,
    ) -> Result<Vec<(FillSeq, FillRecord)>, CursorNotFound> {
        bench_scopes!("fills", "fills::trades_after");
        use std::ops::Bound;
        let upper = match after {
            None => Bound::Included(FillByUserKey::last(user)),
            Some(fill_id) => {
                let key = FillByUserKey {
                    user,
                    seq: fill_id.seq(),
                };
                if !self.by_user.contains_key(&key) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(key)
            }
        };
        Ok(self
            .by_user
            .range((Bound::Included(FillByUserKey::first(user)), upper))
            .rev()
            .take(length)
            .map(|entry| {
                let fill_id = entry.value();
                let record = self
                    .fills
                    .get(&fill_id)
                    .expect("BUG: by_user index references a missing fill record");
                (fill_id.seq(), record)
            })
            .collect())
    }

    #[cfg(test)]
    fn len(&self) -> u64 {
        self.fills.len()
    }

    #[cfg(test)]
    fn next_seq(&self) -> FillSeq {
        FillSeq::new(*self.next_seq.get())
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (FillId, FillRecord)> + '_ {
        self.fills.iter().map(|entry| (*entry.key(), entry.value()))
    }

    #[cfg(test)]
    fn user_index_iter(&self) -> impl Iterator<Item = (FillByUserKey, FillId)> + '_ {
        self.by_user
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for FillStore<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (id, record) in self.iter() {
            assert_eq!(fresh.fills.insert(id, record), None);
        }
        for (key, fill_id) in self.user_index_iter() {
            assert_eq!(fresh.by_user.insert(key, fill_id), None);
        }
        fresh.next_seq.set(*self.next_seq.get());
        fresh
    }
}

#[cfg(test)]
impl PartialEq for FillStore<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.next_seq.get() == other.next_seq.get()
            && self.iter().eq(other.iter())
            && self.user_index_iter().eq(other.user_index_iter())
    }
}

#[cfg(test)]
impl Eq for FillStore<ic_stable_structures::VectorMemory> {}
