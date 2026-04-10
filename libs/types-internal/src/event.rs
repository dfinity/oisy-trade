use crate::{InitArg, UpgradeArg};
use candid::{CandidType, Principal};
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
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct AddTradingPairEvent {
    pub base: Principal,
    pub quote: Principal,
    pub tick_size: u64,
    pub lot_size: u64,
    pub base_symbol: String,
    pub base_decimals: u8,
    pub quote_symbol: String,
    pub quote_decimals: u8,
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
