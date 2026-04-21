use crate::order::{
    LotSize, OrderBookId, OrderId, OrderSeq, Price, Quantity, Side, TickSize, TokenId,
    TokenMetadata,
};
use candid::Principal;
use dex_types_internal::{InitArg, UpgradeArg};
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use minicbor::{Decode, Encode};
use std::borrow::Cow;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct Event {
    #[n(0)]
    pub timestamp: u64,
    #[n(1)]
    pub payload: EventType,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub enum EventType {
    #[n(0)]
    Init(#[n(0)] InitArg),
    #[n(1)]
    Upgrade(#[n(0)] UpgradeArg),
    #[n(2)]
    AddTradingPair(#[n(0)] AddTradingPairEvent),
    #[n(3)]
    Deposit(#[n(0)] DepositEvent),
    #[n(4)]
    AddLimitOrder(#[n(0)] AddLimitOrderEvent),
    #[n(5)]
    Matching(#[n(0)] MatchingEvent),
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct AddTradingPairEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub base: TokenId,
    #[n(2)]
    pub quote: TokenId,
    #[n(3)]
    pub tick_size: TickSize,
    #[n(4)]
    pub lot_size: LotSize,
    #[n(5)]
    pub base_metadata: TokenMetadata,
    #[n(6)]
    pub quote_metadata: TokenMetadata,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct DepositEvent {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub user: Principal,
    #[n(1)]
    pub token: TokenId,
    #[n(2)]
    pub amount: Quantity,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct AddLimitOrderEvent {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub user: Principal,
    #[n(1)]
    pub order_id: OrderId,
    #[n(2)]
    pub side: Side,
    #[n(3)]
    pub price: Price,
    #[n(4)]
    pub quantity: Quantity,
}

/// One matching round on a single [`OrderBookId`], grouping every fill that
/// occurred in that round. Principals, prices, and trading pair are *not*
/// duplicated — they are recoverable from [`crate::order::OrderHistory`] and
/// [`crate::state::State::trading_pairs`] via `(book_id, order_seq)`.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct MatchingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub fills: Vec<FillEvent>,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct FillEvent {
    #[n(0)]
    pub maker_order_seq: OrderSeq,
    #[n(1)]
    pub taker_order_seq: OrderSeq,
    /// Taker side — direction of the trade from the aggressor's point of view.
    #[n(2)]
    pub side: Side,
    #[n(3)]
    pub filled_quantity: Quantity,
}

impl Storable for Event {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = vec![];
        minicbor::encode(self, &mut buf).expect("event encoding should always succeed");
        Cow::Owned(buf)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buf = vec![];
        minicbor::encode(&self, &mut buf).expect("event encoding should always succeed");
        buf
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(bytes.as_ref())
            .unwrap_or_else(|e| panic!("failed to decode event bytes: {e}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}
