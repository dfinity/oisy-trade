use crate::order::{
    LotSize, OrderBookId, OrderId, OrderSeq, OrderStatus, Price, Quantity, Side, TickSize, TokenId,
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
    Settling(#[n(0)] SettlingEvent),
    #[n(6)]
    Matching(#[n(0)] MatchingEvent),
    #[n(7)]
    Withdraw(#[n(0)] WithdrawEvent),
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

/// A successful withdrawal: `amount` of `token` was debited from `user`'s free
/// balance and the corresponding ledger transfer to the user's account
/// completed. Failed withdrawals (ledger errors) do not appear in the log.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct WithdrawEvent {
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

/// Orders processed by the matching engine.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct MatchingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub orders: Vec<OrderSeq>,
}

/// Outcome of the matching engine:
/// * balance transitions between maker/taker
/// * order transitions
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct SettlingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub balance_operations: Vec<BalanceOperation>,
    #[n(2)]
    pub transitions: Vec<OrderStatusTransition>,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub enum BalanceOperation {
    #[n(0)]
    Transfer {
        #[n(0)]
        from: OrderSeq,
        #[n(1)]
        to: OrderSeq,
        #[n(2)]
        token: PairToken,
        #[n(3)]
        amount: Quantity,
    },
    /// Today's only producer is the buy-taker price-improvement refund
    /// (always quote). The `token` field stays explicit so a future cancel
    /// flow can unreserve base as well.
    #[n(1)]
    Unreserve {
        #[n(0)]
        user: OrderSeq,
        #[n(1)]
        token: PairToken,
        #[n(2)]
        amount: Quantity,
    },
}

/// Side of a trading pair for a [`BalanceOperation`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Decode, Encode)]
pub enum PairToken {
    #[n(0)]
    Base,
    #[n(1)]
    Quote,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct OrderStatusTransition {
    #[n(0)]
    pub seq: OrderSeq,
    #[n(1)]
    pub status: OrderStatus,
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
