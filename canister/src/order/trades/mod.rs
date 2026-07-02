use super::{FillSeq, OrderId, PairToken, Price, Quantity, Side, TradeId};
use crate::Timestamp;
use crate::history::{CursorNotFound, History};
use crate::user::UserId;
use ic_stable_structures::Memory;
use std::fmt;

#[cfg(test)]
mod tests;

/// One side-projected trade, holding everything needed to audit one of the two
/// orders' view of a match. The owning `order_id` and the match's `fill_seq`
/// live in the [`TradeId`] key, never in the value; the counterparty is never
/// stored.
///
/// Once the canister is launched its CBOR layout is an upgrade-durable schema;
/// pre-launch there are no persisted records, so schema changes are acceptable.
#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct TradeRecord {
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

impl TradeRecord {
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

/// One side-projected trade together with its primary key — what settlement
/// produces and [`TradeHistory::append`] consumes.
pub type TradeLeg = (TradeId, TradeRecord);

/// Append-only store of side-projected trade records, built on the shared
/// [`History`] core: a primary map keyed by an `OrderId`-prefixed [`TradeId`]
/// (so a per-order read is a prefix range scan, no separate by-order index) plus
/// the core's per-user secondary index for the account-wide read. The two
/// side-projected records of one match share the book-minted `FillSeq` in their
/// [`TradeId`]s and differ by `OrderId`.
pub struct TradeHistory<M: Memory>(History<M, TradeId, TradeRecord>);

impl<M: Memory> fmt::Debug for TradeHistory<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TradeHistory")
            .field("len", &self.0.len())
            .finish()
    }
}

impl<M: Memory> TradeHistory<M> {
    /// `trades_memory` and `by_user_memory` **must be two distinct memory
    /// regions**.
    pub fn new(trades_memory: M, by_user_memory: M) -> Self {
        Self(History::new(trades_memory, by_user_memory))
    }

    /// Append a match's two side-projected records — the taker leg and the maker
    /// leg, each owned by the order's resolved owner. Each leg is keyed by its
    /// [`TradeId`] (the match's shared `FillSeq` paired with each owning
    /// `OrderId`) and stamped with the settle-time. Each record is written to the
    /// primary map and indexed under its owner (2 + 2 inserts per match).
    pub fn append(
        &mut self,
        taker_leg: TradeLeg,
        taker_user: UserId,
        maker_leg: TradeLeg,
        maker_user: UserId,
    ) {
        bench_scopes!("fills", "fills::append");
        let (taker_id, taker_trade) = taker_leg;
        let (maker_id, maker_trade) = maker_leg;
        self.0.insert_once(taker_user, taker_id, taker_trade);
        self.0.insert_once(maker_user, maker_id, maker_trade);
    }

    /// Returns up to `length` of `order`'s trades, newest first. With
    /// `after: None` the page starts at the newest trade; otherwise `after` is a
    /// cursor — the [`TradeId`] of the previous page's last trade — and the page
    /// continues with the next-older trade. An `after` that is not one of
    /// `order`'s trades — including one whose embedded `OrderId` names a different
    /// order (e.g. the counterparty leg sharing the match's `FillSeq`) — yields
    /// [`CursorNotFound`]; a valid cursor with no older trades is `Ok(vec![])`.
    /// The per-order read is a prefix range scan over the primary map, exploiting
    /// `TradeId`'s `OrderId` prefix.
    pub fn trades_for_order(
        &self,
        order: OrderId,
        after: Option<TradeId>,
        length: usize,
    ) -> Result<Vec<(FillSeq, TradeRecord)>, CursorNotFound> {
        bench_scopes!("fills", "fills::trades_for_order");
        use std::ops::Bound;
        let upper = match after {
            None => Bound::Included(TradeId::last_of(order)),
            Some(id) => {
                if id.order_id() != order || !self.0.contains_key(&id) {
                    return Err(CursorNotFound);
                }
                Bound::Excluded(id)
            }
        };
        Ok(self
            .0
            .range_primary(TradeId::first_of(order), upper, length)
            .into_iter()
            .map(|(id, trade)| (id.seq(), trade))
            .collect())
    }

    /// Returns up to `length` of `user`'s trades across **all** their orders,
    /// newest first. With `after: None` the page starts at the newest trade;
    /// otherwise `after` is a cursor — the last trade of the previous page — and
    /// the page continues with the next-older trade. An `after` whose `TradeId`
    /// is not one of `user`'s trades yields [`CursorNotFound`]; a valid cursor
    /// with no older trades is `Ok(vec![])`. Each page reverse-scans the per-user
    /// index and resolves each [`TradeId`] from the primary map, so it is
    /// `O(length * log n)`.
    pub fn trades_after(
        &self,
        user: UserId,
        after: Option<TradeId>,
        length: usize,
    ) -> Result<Vec<(TradeId, TradeRecord)>, CursorNotFound> {
        bench_scopes!("fills", "fills::trades_after");
        let ids = self.0.page_by_user(user, after, length)?;
        Ok(ids
            .into_iter()
            .map(|id| {
                let trade = self
                    .0
                    .get(&id)
                    .expect("BUG: by_user index references a missing trade");
                (id, trade)
            })
            .collect())
    }
}

#[cfg(test)]
impl TradeHistory<ic_stable_structures::VectorMemory> {
    fn len(&self) -> u64 {
        self.0.len()
    }
}

#[cfg(test)]
impl Clone for TradeHistory<ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[cfg(test)]
impl PartialEq for TradeHistory<ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
impl Eq for TradeHistory<ic_stable_structures::VectorMemory> {}
