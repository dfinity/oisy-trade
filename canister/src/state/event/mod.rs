use crate::Timestamp;
use crate::order::{
    FeeRates, LotSize, OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side, TickSize,
    TimeInForce, TokenId, TokenMetadata,
};
use crate::settlement::FillEvent;
use candid::Principal;
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use minicbor::{Decode, Encode};
use oisy_trade_types_internal::{InitArg, UpgradeArg};
use std::borrow::Cow;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct Event {
    #[n(0)]
    pub timestamp: Timestamp,
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
    #[n(8)]
    CancelLimitOrder(#[n(0)] CancelLimitOrderEvent),
    #[n(9)]
    SetHalt(#[n(0)] SetHaltEvent),
    #[n(10)]
    AddTradingAccount(#[n(0)] AddTradingAccountEvent),
    #[n(11)]
    RemoveTradingAccount(#[n(0)] RemoveTradingAccountEvent),
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
    #[n(7)]
    pub fee_rates: FeeRates,
    #[n(8)]
    pub min_notional: Quantity,
    #[n(9)]
    pub max_notional: Option<Quantity>,
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
/// completed at `block_index`. Failed withdrawals (ledger errors) do not
/// appear in the log.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct WithdrawEvent {
    #[n(0)]
    pub block_index: u64,
    #[cbor(n(1), with = "icrc_cbor::principal")]
    pub user: Principal,
    #[n(2)]
    pub token: TokenId,
    #[n(3)]
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
    /// Time-in-force policy.
    #[n(5)]
    pub time_in_force: TimeInForce,
}

/// Orders processed by the matching engine.
#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct MatchingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub orders: Vec<OrderSeq>,
}

/// Outcome of the matching engine, drained in the settling phase:
/// * `balance_operations`: balance transitions between maker/taker;
/// * `fills`: the lean per-fill records whose two side-projected trade records
///   are rebuilt and persisted to `TradeHistory` in the settling phase — the
///   side and execution price are recovered from the order records and the
///   notional/fees recomputed. Empty for cancel-driven settling events, which
///   carry only an unreserve operation.
#[derive(Clone, PartialEq, Eq, Debug, Decode, Encode)]
pub struct SettlingEvent {
    #[n(0)]
    pub book_id: OrderBookId,
    #[n(1)]
    pub balance_operations: Vec<BalanceOperation>,
    #[n(2)]
    pub fills: Vec<FillEvent>,
}

/// Participants are identified by `OrderSeq` — the apply path resolves each
/// seq to a `Principal` via `OrderHistory`. `token` is a `PairToken` selector
/// resolved to a concrete `TokenId` via the enclosing `SettlingEvent`'s
/// `book_id`. This keeps each op compact on the wire while still reconstructing
/// enough context at apply time.
#[derive(Clone, PartialEq, Eq, Debug, Decode, Encode)]
pub enum BalanceOperation {
    #[n(0)]
    Transfer {
        #[n(0)]
        from_order: OrderSeq,
        #[n(1)]
        to_order: OrderSeq,
        #[n(2)]
        token: PairToken,
        /// Gross amount transferred from the debtor's reserved balance.
        /// The creditor receives `amount - fee.unwrap_or(ZERO)`; the
        /// remainder accrues to the canister-owned fee pool of `token`.
        #[n(3)]
        amount: Quantity,
        /// Fee withheld for the per-token fee pool. `None` is interpreted
        /// as zero (no fee).
        #[n(4)]
        fee: Option<Quantity>,
    },
    /// Producers: the buy-taker price-improvement refund (always quote) and
    /// the cancel-limit-order flow (quote for Buy, base for Sell). The
    /// `token` field is explicit because the cancel side can unreserve
    /// either token.
    #[n(1)]
    Unreserve {
        #[n(0)]
        order: OrderSeq,
        #[n(1)]
        token: PairToken,
        #[n(2)]
        amount: Quantity,
    },
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct CancelLimitOrderEvent {
    #[n(0)]
    pub order_id: OrderId,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct SetHaltEvent {
    #[n(0)]
    pub book_ids: Option<Vec<OrderBookId>>,
    #[n(1)]
    pub halted: bool,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct AddTradingAccountEvent {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub funding: Principal,
    #[cbor(n(1), with = "icrc_cbor::principal")]
    pub trading: Principal,
}

#[derive(Clone, PartialEq, Debug, Decode, Encode)]
pub struct RemoveTradingAccountEvent {
    #[cbor(n(0), with = "icrc_cbor::principal")]
    pub funding: Principal,
    #[cbor(n(1), with = "icrc_cbor::principal")]
    pub trading: Principal,
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
