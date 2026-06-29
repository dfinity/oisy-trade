use super::{FillSeq, OrderId, PairToken, Price, Quantity, Side, TradeId};
use crate::Timestamp;
use crate::user::UserId;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{Memory, StableBTreeMap, Storable};
use std::borrow::Cow;
use std::fmt;

#[cfg(test)]
mod tests;

/// Key into the account-wide secondary index: the interned [`UserId`] followed
/// by a canister-global insertion sequence, so a range scan over a user's prefix
/// yields their trades oldest-first across all their orders —
/// [`TradeHistory::trades_after`] reverses it for newest-first. The value is the
/// [`TradeId`], pointing back into the primary `trades` map.
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TradeByUserKey {
    user: UserId,
    seq: u64,
}

/// 8 bytes of `UserId` + 8 bytes of `seq`, both big-endian.
const TRADE_BY_USER_KEY_LEN: usize = 8 + 8;

impl TradeByUserKey {
    fn from_seq(user: UserId, seq: u64) -> Self {
        Self { user, seq }
    }

    fn first(user: UserId) -> Self {
        Self { user, seq: 0 }
    }

    fn last(user: UserId) -> Self {
        Self {
            user,
            seq: u64::MAX,
        }
    }
}

impl Storable for TradeByUserKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; TRADE_BY_USER_KEY_LEN];
        buf[..8].copy_from_slice(&self.user.get().to_be_bytes());
        buf[8..].copy_from_slice(&self.seq.to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            TRADE_BY_USER_KEY_LEN,
            "TradeByUserKey must decode from exactly {TRADE_BY_USER_KEY_LEN} bytes"
        );
        let user = UserId::new(u64::from_be_bytes(
            bytes[..8].try_into().expect("8-byte slice"),
        ));
        let seq = u64::from_be_bytes(bytes[8..].try_into().expect("8-byte slice"));
        Self { user, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: TRADE_BY_USER_KEY_LEN as u32,
        is_fixed_size: true,
    };
}

/// One side-projected trade, holding everything needed to audit one of the two
/// orders' view of a match. The owning `order_id` and the match's `fill_seq`
/// live in the [`TradeId`] key, never in the value; the counterparty is never
/// stored.
///
/// Once the canister is launched its CBOR layout is an upgrade-durable schema;
/// pre-launch there are no persisted records, so schema changes are acceptable.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct Trade {
    #[n(0)]
    pub side: Side,
    #[n(1)]
    pub price: Price,
    #[n(2)]
    pub quantity: Quantity,
    #[n(3)]
    pub notional: Quantity,
    #[n(4)]
    pub fee: Quantity,
    #[n(5)]
    pub fee_token: PairToken,
    #[n(6)]
    pub is_maker: bool,
    #[n(7)]
    pub timestamp: Timestamp,
}

impl Storable for Trade {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("trade encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("trade encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode trade bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Trade {
    /// Projects this trade to the public [`oisy_trade_types::Trade`], stamping it
    /// with `id` as its identity and pagination cursor — its [`TradeId`] in the
    /// same opaque text form `get_my_trades` decodes for `after`, so a returned
    /// id round-trips.
    pub fn into_public(self, id: TradeId) -> oisy_trade_types::Trade {
        oisy_trade_types::Trade {
            id: id.to_string(),
            order_id: id.order_id().to_string(),
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

/// Stored value of [`TradeHistory`]'s primary map: a [`Trade`] paired with the
/// canister-global insertion sequence assigned when it was inserted. That
/// sequence keys the per-user index (scanned in reverse for newest-first) and
/// lets `trades_after` resolve a [`TradeId`] cursor back to its index position
/// in O(log n). It's an index bookkeeping concern, so it lives in this wrapper
/// rather than as a field on the domain [`Trade`]. Mirrors
/// [`crate::order::OrderHistory`]'s `SeqOrderRecord`.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
struct SeqTrade {
    #[n(0)]
    global_seq: u64,
    #[n(1)]
    trade: Trade,
}

impl Storable for SeqTrade {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("seq trade encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("seq trade encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode seq trade bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

/// One side-projected trade together with its primary key — what settlement
/// produces and [`TradeHistory::append`] consumes.
pub type TradeLeg = (TradeId, Trade);

/// The `after` cursor passed to a reader names a trade that is unknown (no
/// record with that sequence in the scanned prefix) or not owned by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorNotFound;

/// Append-only store of side-projected trade records, mirroring the storage
/// shape of [`crate::order::OrderHistory`]: a primary map keyed by an
/// `OrderId`-prefixed [`TradeId`] (so a per-order read is a prefix range scan,
/// no separate by-order index) plus a `(UserId, global_seq)` secondary index for
/// the account-wide read. The two side-projected records of one match share the
/// book-minted `FillSeq` in their [`TradeId`]s and differ by `OrderId`.
pub struct TradeHistory<M: Memory> {
    trades: StableBTreeMap<TradeId, SeqTrade, M>,
    by_user: StableBTreeMap<TradeByUserKey, TradeId, M>,
}

impl<M: Memory> fmt::Debug for TradeHistory<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TradeHistory")
            .field("len", &self.trades.len())
            .finish()
    }
}

impl<M: Memory> TradeHistory<M> {
    /// `trades_memory` and `by_user_memory` **must be two distinct memory
    /// regions**.
    pub fn new(trades_memory: M, by_user_memory: M) -> Self {
        Self {
            trades: StableBTreeMap::init(trades_memory),
            by_user: StableBTreeMap::init(by_user_memory),
        }
    }

    /// Append the two side-projected records of one match — the taker leg owned
    /// by `taker_user` and the maker leg owned by `maker_user`, both already
    /// keyed by their [`TradeId`] (the match's shared `FillSeq` paired with each
    /// owning `OrderId`). Each record is written to the primary map and indexed
    /// under its owner in `by_user` (2 + 2 inserts per match).
    pub fn append(
        &mut self,
        taker_leg: TradeLeg,
        taker_user: UserId,
        maker_leg: TradeLeg,
        maker_user: UserId,
    ) {
        bench_scopes!("fills", "fills::append");
        self.insert(taker_leg, taker_user);
        self.insert(maker_leg, maker_user);
    }

    fn insert(&mut self, leg: TradeLeg, user: UserId) {
        let (id, trade) = leg;
        let global_seq = self.by_user.len();
        assert_eq!(
            self.trades.insert(id, SeqTrade { global_seq, trade }),
            None,
            "BUG: duplicate trade id {id:?}"
        );
        assert_eq!(
            self.by_user
                .insert(TradeByUserKey::from_seq(user, global_seq), id),
            None,
            "BUG: duplicate user-trade index entry for {user:?} seq {global_seq}"
        );
    }

    /// Returns up to `length` of `order`'s trades, newest first. With
    /// `after: None` the page starts at the newest trade; otherwise `after` is a
    /// cursor — the last trade of the previous page — and the page continues with
    /// the next-older trade. An `after` whose sequence is not one of `order`'s
    /// trades yields [`CursorNotFound`]; a valid cursor with no older trades is
    /// `Ok(vec![])`.
    pub fn trades_for_order(
        &self,
        order: OrderId,
        after: Option<FillSeq>,
        length: usize,
    ) -> Result<Vec<(FillSeq, Trade)>, CursorNotFound> {
        bench_scopes!("fills", "fills::trades_for_order");
        use std::ops::Bound;
        let upper = match after {
            None => Bound::Included(TradeId::last_of(order)),
            Some(seq) => {
                let id = TradeId::new(order, seq);
                if !self.trades.contains_key(&id) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(id)
            }
        };
        Ok(self
            .trades
            .range((Bound::Included(TradeId::first_of(order)), upper))
            .rev()
            .take(length)
            .map(|entry| (entry.key().seq(), entry.value().trade))
            .collect())
    }

    /// Returns up to `length` of `user`'s trades across **all** their orders,
    /// newest first. With `after: None` the page starts at the newest trade;
    /// otherwise `after` is a cursor — the last trade of the previous page — and
    /// the page continues with the next-older trade. An `after` whose `TradeId`
    /// is not one of `user`'s trades yields [`CursorNotFound`]; a valid cursor
    /// with no older trades is `Ok(vec![])`. The cursor's index position is
    /// resolved via an O(log n) point lookup of its stored `global_seq` in the
    /// primary map; each page then reverse-scans the `by_user` index and resolves
    /// each [`TradeId`] from the primary map — the exact shape of
    /// `OrderHistory::orders_after` — so it is `O(length)`.
    pub fn trades_after(
        &self,
        user: UserId,
        after: Option<TradeId>,
        length: usize,
    ) -> Result<Vec<(TradeId, Trade)>, CursorNotFound> {
        bench_scopes!("fills", "fills::trades_after");
        use std::ops::Bound;
        let upper = match after {
            None => Bound::Included(TradeByUserKey::last(user)),
            Some(cursor) => {
                let entry = self.trades.get(&cursor).ok_or(CursorNotFound)?;
                let key = TradeByUserKey::from_seq(user, entry.global_seq);
                if self.by_user.get(&key) != Some(cursor) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(key)
            }
        };
        Ok(self
            .by_user
            .range((Bound::Included(TradeByUserKey::first(user)), upper))
            .rev()
            .take(length)
            .map(|entry| {
                let id = entry.value();
                let trade = self
                    .trades
                    .get(&id)
                    .expect("BUG: by_user index references a missing trade")
                    .trade;
                (id, trade)
            })
            .collect())
    }

    #[cfg(test)]
    fn len(&self) -> u64 {
        self.trades.len()
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (TradeId, SeqTrade)> + '_ {
        self.trades
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }

    #[cfg(test)]
    fn user_index_iter(&self) -> impl Iterator<Item = (TradeByUserKey, TradeId)> + '_ {
        self.by_user
            .iter()
            .map(|entry| (*entry.key(), entry.value()))
    }
}

#[cfg(test)]
impl Clone for TradeHistory<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let mut fresh = Self::new(
            ic_stable_structures::VectorMemory::default(),
            ic_stable_structures::VectorMemory::default(),
        );
        for (id, trade) in self.iter() {
            assert_eq!(fresh.trades.insert(id, trade), None);
        }
        for (key, id) in self.user_index_iter() {
            assert_eq!(fresh.by_user.insert(key, id), None);
        }
        fresh
    }
}

#[cfg(test)]
impl PartialEq for TradeHistory<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter()) && self.user_index_iter().eq(other.user_index_iter())
    }
}

#[cfg(test)]
impl Eq for TradeHistory<ic_stable_structures::VectorMemory> {}
