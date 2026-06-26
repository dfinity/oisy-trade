use super::{OrderId, PairToken, Price, Quantity, Side};
use crate::Timestamp;
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
/// globally and within an order's prefix. It is the `after` cursor exposed to
/// callers as an opaque text token.
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

impl fmt::Display for FillSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// A [`FillSeq`] cursor was not a well-formed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillSeqParseError;

impl FromStr for FillSeq {
    type Err = FillSeqParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 || !s.is_ascii() {
            return Err(FillSeqParseError);
        }
        u64::from_str_radix(s, 16)
            .map(Self)
            .map_err(|_| FillSeqParseError)
    }
}

/// Key into the primary fill map: the owning [`OrderId`] followed by the
/// canister-global [`FillSeq`]. A range scan over an `order` prefix yields that
/// order's fills in `seq` order; reversed, newest-first.
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FillKey {
    order: OrderId,
    seq: FillSeq,
}

/// 16 bytes of `OrderId` + 8 bytes of `seq`, both big-endian.
const FILL_KEY_LEN: usize = 16 + 8;

impl FillKey {
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

impl Storable for FillKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; FILL_KEY_LEN];
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
            FILL_KEY_LEN,
            "FillKey must decode from exactly {FILL_KEY_LEN} bytes"
        );
        let order = OrderId::from_bytes(Cow::Borrowed(&bytes[..16]));
        let seq = FillSeq::new(u64::from_be_bytes(
            bytes[16..].try_into().expect("8-byte slice"),
        ));
        Self { order, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: FILL_KEY_LEN as u32,
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

impl FillRecord {
    /// Projects this record to the public [`oisy_trade_types::Trade`], stamping
    /// it with `seq` as its pagination cursor — encoded with the same scheme
    /// `get_my_trades` decodes for `after`, so a returned cursor round-trips.
    pub fn into_trade(self, seq: FillSeq) -> oisy_trade_types::Trade {
        oisy_trade_types::Trade {
            cursor: seq.to_string(),
            order_id: self.order_id.into(),
            side: self.side.into(),
            price: candid::Nat::from(self.price),
            quantity: self.quantity.into(),
            notional: self.notional.into(),
            fee: self.fee.into(),
            fee_token: self.fee_token.into(),
            is_maker: self.is_maker,
            timestamp: self.timestamp.as_nanos(),
        }
    }
}

/// The `after` cursor passed to [`FillStore::fills_after`] names a fill that is
/// unknown (no record with that global sequence in the order's prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorNotFound;

/// Append-only store of side-projected fill records, mirroring the storage
/// shape of [`crate::order::OrderHistory`]: a primary map keyed by an
/// `OrderId`-prefixed key (so a per-order read is a range scan), plus a
/// canister-global monotonic [`FillSeq`] counter persisted in its own cell so
/// it stays monotonic across upgrades.
///
/// The account-wide secondary index (`by_user`) and its reader come in a
/// follow-up; this store implements the per-order feed only.
pub struct FillStore<M: Memory> {
    fills: StableBTreeMap<FillKey, FillRecord, M>,
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
    /// `fills_memory` and `seq_memory` **must be distinct memory regions**.
    pub fn new(fills_memory: M, seq_memory: M) -> Self {
        Self {
            fills: StableBTreeMap::init(fills_memory),
            next_seq: StableCell::init(seq_memory, 0),
        }
    }

    /// Append the two side-projected records of one fill — the taker leg and the
    /// maker leg — each under the next global [`FillSeq`], advancing the
    /// sequence by two.
    pub fn append(&mut self, taker_leg: FillRecord, maker_leg: FillRecord) {
        bench_scopes!("fills", "fills::append");
        self.insert(taker_leg);
        self.insert(maker_leg);
    }

    fn insert(&mut self, record: FillRecord) {
        let seq = FillSeq::new(*self.next_seq.get());
        let key = FillKey {
            order: record.order_id,
            seq,
        };
        assert_eq!(
            self.fills.insert(key, record),
            None,
            "BUG: duplicate fill key for seq {seq}"
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
            None => Bound::Included(FillKey::last(order)),
            Some(seq) => {
                let key = FillKey { order, seq };
                if !self.fills.contains_key(&key) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(key)
            }
        };
        Ok(self
            .fills
            .range((Bound::Included(FillKey::first(order)), upper))
            .rev()
            .take(length)
            .map(|entry| (entry.key().seq, entry.value()))
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
    fn iter(&self) -> impl Iterator<Item = (FillKey, FillRecord)> + '_ {
        self.fills.iter().map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for FillStore<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (key, record) in self.iter() {
            assert_eq!(fresh.fills.insert(key, record), None);
        }
        fresh.next_seq.set(*self.next_seq.get());
        fresh
    }
}

#[cfg(test)]
impl PartialEq for FillStore<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.next_seq.get() == other.next_seq.get() && self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl Eq for FillStore<ic_stable_structures::VectorMemory> {}
