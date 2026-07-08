use crate::{InitArg, UpgradeArg};
use candid::{CandidType, Nat, Principal};
use oisy_trade_types::{PairToken, TimeInForce, TokenId, TokenMetadata};
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
    SetHalt(SetHaltEvent),
    AddTradingAccount(AddTradingAccountEvent),
    RemoveTradingAccount(RemoveTradingAccountEvent),
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
    pub side: oisy_trade_types::Side,
    pub price: Nat,
    pub quantity: Nat,
    pub time_in_force: TimeInForce,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct CancelLimitOrderEvent {
    pub order_id: OrderId,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct SetHaltEvent {
    pub book_ids: Option<Vec<u64>>,
    pub halted: bool,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct AddTradingAccountEvent {
    pub funding: Principal,
    pub trading: Principal,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct RemoveTradingAccountEvent {
    pub funding: Principal,
    pub trading: Principal,
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
    pub fills: Vec<FillEvent>,
}

#[derive(Clone, Debug, PartialEq, CandidType, Deserialize)]
pub struct FillEvent {
    pub fill_seq: u64,
    pub taker_order_seq: u64,
    pub maker_order_seq: u64,
    pub quantity: Nat,
    pub maker_fee_bps: u16,
    pub taker_fee_bps: u16,
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
