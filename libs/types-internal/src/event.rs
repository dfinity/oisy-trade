use crate::{InitArg, UpgradeArg};
use candid::CandidType;
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
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct AddTradingPairEvent {
    pub book_id: u64,
    pub base: TokenId,
    pub quote: TokenId,
    pub tick_size: u64,
    pub lot_size: u64,
    pub base_metadata: TokenMetadata,
    pub quote_metadata: TokenMetadata,
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
