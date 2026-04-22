use crate::order::{
    LotSize, MatchingOutput, OrderBookId, OrderId, OrderSeq, Price, Quantity, Side, TickSize,
    TokenId, TokenMetadata,
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
    // #[n(5)] keeps its existing wire shape (book_id + MatchingOutput); only
    // the Rust variant name changes to reflect its narrower concern.
    #[n(5)]
    Settling(#[n(0)] SettlingEvent),
    #[n(6)]
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

/// Authoritative record of which orders the engine processed in a matching
/// round on `book_id`. Drives `record_matching_event`'s apply: replay pops
/// exactly these sequences from the book's pending queue, in order.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct MatchingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub orders: Vec<OrderSeq>,
}

/// Settlement + status outcome of the matching round on `book_id`.
/// `record_settling_event` drains [`crate::state::State::pending_settlement`]
/// (populated by the preceding [`MatchingEvent`]'s apply) and asserts the
/// stored value matches `output`, catching primary/replay drift.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct SettlingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub output: MatchingOutput,
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
