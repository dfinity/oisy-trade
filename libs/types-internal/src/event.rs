use crate::{InitArg, UpgradeArg};
use candid::{CandidType, Nat, Principal};
use dex_types::{TokenId, TokenMetadata};
use serde::Deserialize;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Event {
    pub timestamp: u64,
    pub payload: EventType,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum EventType {
    Init(InitArg),
    Upgrade(UpgradeArg),
    AddTradingPair(AddTradingPairEvent),
    Deposit(DepositEvent),
    AddLimitOrder(AddLimitOrderEvent),
    Settling(SettlingEvent),
    Matching(MatchingEvent),
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct AddTradingPairEvent {
    pub book_id: u64,
    pub base: TokenId,
    pub quote: TokenId,
    pub tick_size: u64,
    pub lot_size: u64,
    pub base_metadata: TokenMetadata,
    pub quote_metadata: TokenMetadata,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct DepositEvent {
    pub user: Principal,
    pub token: TokenId,
    pub amount: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct AddLimitOrderEvent {
    pub user: Principal,
    pub order_id: OrderId,
    pub side: dex_types::Side,
    pub price: u64,
    pub quantity: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct MatchingEvent {
    pub book_id: u64,
    pub orders: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct SettlingEvent {
    pub book_id: u64,
    pub balance_operations: Vec<BalanceOperation>,
    pub transitions: Vec<OrderStatusTransition>,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub enum BalanceOperation {
    Transfer {
        from: u64,
        to: u64,
        token: PairToken,
        amount: Nat,
    },
    Unreserve {
        user: u64,
        token: PairToken,
        amount: Nat,
    },
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub enum PairToken {
    Base,
    Quote,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct OrderStatusTransition {
    pub seq: u64,
    pub status: dex_types::OrderStatus,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct MatchingOutput {
    pub fills: Vec<Fill>,
    pub resting_orders: Vec<u64>,
    pub filled_orders: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct Fill {
    pub taker_order_seq: u64,
    pub taker_side: dex_types::Side,
    pub taker_price: u64,
    pub maker_order_seq: u64,
    pub maker_price: u64,
    pub quantity: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct OrderId {
    pub book_id: u64,
    pub seq: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetEventsArgs {
    pub start: u64,
    pub length: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetEventsResult {
    pub events: Vec<Event>,
    pub total_event_count: u64,
}
