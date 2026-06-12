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
    CancelLimitOrder(CancelLimitOrderEvent),
    Settling(SettlingEvent),
    Matching(MatchingEvent),
    Withdraw(WithdrawEvent),
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct AddTradingPairEvent {
    pub book_id: u64,
    pub base: TokenId,
    pub quote: TokenId,
    pub tick_size: Nat,
    pub lot_size: Nat,
    pub base_metadata: TokenMetadata,
    pub quote_metadata: TokenMetadata,
    pub maker_fee_bps: u16,
    pub taker_fee_bps: u16,
    pub min_notional: Nat,
    pub max_notional: Option<Nat>,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct DepositEvent {
    pub user: Principal,
    pub token: TokenId,
    pub amount: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct WithdrawEvent {
    pub block_index: u64,
    pub user: Principal,
    pub token: TokenId,
    pub amount: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct AddLimitOrderEvent {
    pub user: Principal,
    pub order_id: OrderId,
    pub side: dex_types::Side,
    pub price: Nat,
    pub quantity: Nat,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct CancelLimitOrderEvent {
    pub order_id: OrderId,
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
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub enum BalanceOperation {
    /// `from_order` / `to_order` are order sequence numbers (`OrderSeq`) that
    /// resolve to principals via the canister's `OrderHistory` at apply time,
    /// not user identifiers.
    Transfer {
        from_order: u64,
        to_order: u64,
        token: PairToken,
        amount: Nat,
        /// Fee withheld from the credited transfer for the per-token
        /// fee pool. `None` means no fee.
        fee: Option<Nat>,
    },
    /// `order` is the order sequence number whose reserved balance is being
    /// released, not a user principal.
    Unreserve {
        order: u64,
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
