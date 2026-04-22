use crate::order::{
    LotSize, OrderBookId, OrderId, OrderSeq, OrderStatus, Price, Quantity, Side, TickSize, TokenId,
    TokenMetadata,
};
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, BalanceOperation, DepositEvent, Event, EventType,
    MatchingEvent, OrderStatusEvent, OrderStatusTransition, PairToken, SettlingEvent,
};
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};

use super::{LOT_SIZE, TICK_SIZE, base_metadata, quote_metadata};

pub fn init_event(mode: Mode) -> Event {
    Event {
        timestamp: 0,
        payload: EventType::Init(InitArg { mode }),
    }
}

pub fn upgrade_event(mode: Option<Mode>) -> Event {
    Event {
        timestamp: 1,
        payload: EventType::Upgrade(UpgradeArg { mode }),
    }
}

pub fn add_trading_pair_event(base: Principal, quote: Principal) -> Event {
    Event {
        timestamp: 2,
        payload: EventType::AddTradingPair(AddTradingPairEvent {
            book_id: OrderBookId::ZERO,
            base: TokenId::new(base),
            quote: TokenId::new(quote),
            tick_size: TICK_SIZE,
            lot_size: LOT_SIZE,
            base_metadata: base_metadata(),
            quote_metadata: quote_metadata(),
        }),
    }
}

/// Adding a new variant to `EventType` will cause a compile error in the `From` impl,
/// reminding you to add corresponding worst-case entries.
#[derive(strum::EnumIter, strum::IntoStaticStr)]
pub enum WorstCaseEvent {
    Init,
    Upgrade,
    AddTradingPair,
    Deposit,
    AddLimitOrder,
    Matching,
    Settling,
    OrderStatus,
}

impl From<&EventType> for WorstCaseEvent {
    fn from(event: &EventType) -> Self {
        match event {
            EventType::Init(_) => Self::Init,
            EventType::Upgrade(_) => Self::Upgrade,
            EventType::AddTradingPair(_) => Self::AddTradingPair,
            EventType::Deposit(_) => Self::Deposit,
            EventType::AddLimitOrder(_) => Self::AddLimitOrder,
            EventType::Matching(_) => Self::Matching,
            EventType::Settling(_) => Self::Settling,
            EventType::OrderStatus(_) => Self::OrderStatus,
        }
    }
}

/// Upper bound on orders processed per matching round, used to size the
/// worst-case `Matching` / `Settling` / `OrderStatus` fixtures. Matches the
/// `bench_process_pending_orders_1000` benchmark workload.
pub const MAX_ORDERS_PER_MATCHING_ROUND: usize = 1_000;

impl WorstCaseEvent {
    /// Event that maximizes serialized byte size in stable memory.
    pub fn worst_case_memory_event(&self) -> Event {
        max_timestamp(match self {
            Self::Init => init_restricted(),
            Self::Upgrade => upgrade_restricted(),
            Self::AddTradingPair => add_trading_pair(),
            Self::Deposit => deposit(max_quantity()),
            Self::AddLimitOrder => add_limit_order(),
            Self::Matching => matching(MAX_ORDERS_PER_MATCHING_ROUND),
            Self::Settling => settling(MAX_ORDERS_PER_MATCHING_ROUND),
            Self::OrderStatus => order_status(MAX_ORDERS_PER_MATCHING_ROUND),
        })
    }

    /// Event that maximizes instruction count during encoding/decoding.
    pub fn worst_case_instructions_event(&self) -> Event {
        // currently same as worst-case memory event
        self.worst_case_memory_event()
    }

    pub fn expected_memory_size(&self) -> usize {
        match self {
            Self::Init => 328,
            Self::Upgrade => 328,
            Self::AddTradingPair => 136,
            Self::Deposit => 95,
            Self::AddLimitOrder => 97,
            Self::Matching => 10_027,
            Self::Settling => 91_327,
            Self::OrderStatus => 14_027,
        }
    }
}

fn max_timestamp(payload: EventType) -> Event {
    Event {
        timestamp: u64::MAX,
        payload,
    }
}

fn restricted_principals() -> std::collections::BTreeSet<Principal> {
    (0u8..10).map(max_principal).collect()
}

fn init_restricted() -> EventType {
    EventType::Init(InitArg {
        mode: Mode::RestrictedTo(restricted_principals()),
    })
}

fn upgrade_restricted() -> EventType {
    EventType::Upgrade(UpgradeArg {
        mode: Some(Mode::RestrictedTo(restricted_principals())),
    })
}

fn add_trading_pair() -> EventType {
    EventType::AddTradingPair(AddTradingPairEvent {
        book_id: OrderBookId::new(u64::MAX),
        base: TokenId::new(max_principal(0)),
        quote: TokenId::new(max_principal(1)),
        tick_size: TickSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
        lot_size: LotSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
        base_metadata: TokenMetadata {
            symbol: max_symbol(),
            decimals: u8::MAX,
        },
        quote_metadata: TokenMetadata {
            symbol: max_symbol(),
            decimals: u8::MAX,
        },
    })
}

fn add_limit_order() -> EventType {
    EventType::AddLimitOrder(AddLimitOrderEvent {
        user: max_principal(0),
        order_id: OrderId::new(OrderBookId::new(u64::MAX), OrderSeq::new(u64::MAX)),
        side: Side::Buy,
        price: Price::new(u64::MAX),
        quantity: max_quantity(),
    })
}

fn deposit(amount: Quantity) -> EventType {
    EventType::Deposit(DepositEvent {
        user: max_principal(0),
        token: TokenId::new(max_principal(1)),
        amount,
    })
}

fn matching(order_count: usize) -> EventType {
    EventType::Matching(MatchingEvent {
        book_id: OrderBookId::new(u64::MAX),
        orders: (0..order_count as u64)
            .map(|i| OrderSeq::new(u64::MAX - i))
            .collect(),
    })
}

fn settling(fill_count: usize) -> EventType {
    // Each fill produces 3 operations (quote transfer, quote unreserve for
    // buy-taker price improvement, base transfer) — the maximum per fill.
    let mut operations = Vec::with_capacity(fill_count * 3);
    for i in 0..fill_count as u64 {
        let taker = OrderSeq::new(2 * i);
        let maker = OrderSeq::new(2 * i + 1);
        operations.push(BalanceOperation::Transfer {
            from: taker,
            to: maker,
            token: PairToken::Quote,
            amount: max_quantity(),
        });
        operations.push(BalanceOperation::Unreserve {
            user: taker,
            token: PairToken::Quote,
            amount: max_quantity(),
        });
        operations.push(BalanceOperation::Transfer {
            from: maker,
            to: taker,
            token: PairToken::Base,
            amount: max_quantity(),
        });
    }
    EventType::Settling(SettlingEvent {
        book_id: OrderBookId::new(u64::MAX),
        operations,
    })
}

fn order_status(order_count: usize) -> EventType {
    let transitions = (0..order_count as u64)
        .map(|i| OrderStatusTransition {
            seq: OrderSeq::new(u64::MAX - i),
            status: OrderStatus::Filled,
        })
        .collect();
    EventType::OrderStatus(OrderStatusEvent {
        book_id: OrderBookId::new(u64::MAX),
        transitions,
    })
}

fn max_principal(seed: u8) -> Principal {
    Principal::from_slice(&[seed; 29])
}

fn max_symbol() -> String {
    "A".repeat(10)
}

fn max_quantity() -> Quantity {
    // EVM-based chains use theoretically u256,
    // but note that for example ETH has a supply of 120 million,
    // which comfortably fits in a u128 (18 decimals).
    Quantity::try_from(candid::Nat::from(u128::MAX)).unwrap()
}
